//! Image-based (environment) lighting.
//!
//! An [`EnvMap`] is an equirectangular (lat-long) HDR image used as a distant
//! light surrounding the whole scene. It supports importance sampling — drawing
//! directions proportional to incoming radiance via a piecewise-constant 2D
//! distribution — so bright regions (a sun, windows) are sampled efficiently and
//! combine with BSDF sampling under MIS.
//!
//! Decoding image files is intentionally *not* done here (that keeps the core
//! codec-free); a front-end loads pixels and calls [`EnvMap::from_pixels`]. A
//! procedural [`EnvMap::sky`] is provided so demos need no external asset.

use crate::color::luminance;
use crate::math::Vec3;
use crate::sampler::Sampler;
use crate::{Color, Float};
use std::f32::consts::PI;

/// Result of importance-sampling an environment map.
pub struct EnvSample {
    /// World-space direction toward the sampled radiance (unit).
    pub dir: Vec3,
    /// Incoming radiance from that direction.
    pub radiance: Color,
    /// Solid-angle pdf of the sample.
    pub pdf: Float,
}

pub struct EnvMap {
    width: usize,
    height: usize,
    pixels: Vec<Color>,
    dist: Distribution2D,
    intensity: Float,
    /// Rotation about the +y axis, in radians.
    rotation: Float,
}

impl EnvMap {
    /// Build an environment from an equirectangular pixel buffer (row-major,
    /// row 0 = top / +y pole). `intensity` scales radiance; `rotation` spins the
    /// map about the vertical axis.
    pub fn from_pixels(
        width: usize,
        height: usize,
        pixels: Vec<Color>,
        intensity: Float,
        rotation: Float,
    ) -> EnvMap {
        assert_eq!(pixels.len(), width * height, "pixel buffer size mismatch");
        // Importance weight = luminance, scaled by sin(theta) so the poles
        // (which cover little solid angle) are not oversampled.
        let mut func = vec![0.0; width * height];
        for v in 0..height {
            let theta = PI * (v as Float + 0.5) / height as Float;
            let sin_theta = theta.sin();
            for u in 0..width {
                func[v * width + u] = luminance(pixels[v * width + u]) * sin_theta;
            }
        }
        let dist = Distribution2D::new(&func, width, height);
        EnvMap {
            width,
            height,
            pixels,
            dist,
            intensity,
            rotation,
        }
    }

    /// A procedural sky: a horizon→zenith gradient with a bright sun disk in
    /// direction `sun_dir`. Asset-free, and importance-sampled like any map.
    pub fn sky(
        width: usize,
        height: usize,
        sun_dir: Vec3,
        sun_color: Color,
        sun_angular_radius: Float,
        horizon: Color,
        zenith: Color,
    ) -> EnvMap {
        let sun = sun_dir.normalize();
        let cos_sun = sun_angular_radius.cos();
        let mut pixels = vec![Color::ZERO; width * height];
        for v in 0..height {
            for u in 0..width {
                let dir = uv_to_dir(
                    (u as Float + 0.5) / width as Float,
                    (v as Float + 0.5) / height as Float,
                );
                let t = (dir.y.max(0.0)).powf(0.5);
                let mut c = horizon * (1.0 - t) + zenith * t;
                if dir.dot(sun) > cos_sun {
                    c = sun_color;
                }
                pixels[v * width + u] = c;
            }
        }
        EnvMap::from_pixels(width, height, pixels, 1.0, 0.0)
    }

    /// Radiance arriving from direction `dir` (for escaped rays / lookups).
    pub fn width(&self) -> usize {
        self.width
    }
    pub fn height(&self) -> usize {
        self.height
    }
    pub fn intensity(&self) -> Float {
        self.intensity
    }
    pub fn rotation(&self) -> Float {
        self.rotation
    }
    /// The raw equirectangular pixels (row 0 = +y pole), for a backend to upload.
    pub fn pixels(&self) -> &[Color] {
        &self.pixels
    }

    pub fn radiance(&self, dir: Vec3) -> Color {
        let (u, v) = dir_to_uv(rotate_y(dir, -self.rotation));
        self.bilinear(u, v) * self.intensity
    }

    /// Solid-angle pdf that [`EnvMap::sample`] would assign to `dir`.
    pub fn pdf(&self, dir: Vec3) -> Float {
        let (u, v) = dir_to_uv(rotate_y(dir, -self.rotation));
        let theta = v * PI;
        let sin_theta = theta.sin();
        if sin_theta <= 0.0 {
            return 0.0;
        }
        self.dist.pdf(u, v) / (2.0 * PI * PI * sin_theta)
    }

    /// Importance-sample an incoming direction.
    pub fn sample(&self, sampler: &mut Sampler) -> Option<EnvSample> {
        let (u1, u2) = sampler.next_2d();
        let ((u, v), map_pdf) = self.dist.sample_continuous(u1, u2);
        if map_pdf <= 0.0 {
            return None;
        }
        let theta = v * PI;
        let sin_theta = theta.sin();
        if sin_theta <= 0.0 {
            return None;
        }
        let dir = rotate_y(uv_to_dir(u, v), self.rotation);
        let pdf = map_pdf / (2.0 * PI * PI * sin_theta);
        Some(EnvSample {
            dir,
            radiance: self.bilinear(u, v) * self.intensity,
            pdf,
        })
    }

    /// Flatten the importance-sampling tables for a GPU backend. Layouts
    /// mirror `Distribution2D`: per-row conditional CDFs (`height × (width+1)`
    /// floats), the marginal CDF (`height + 1`), the sinθ-weighted function
    /// (`height × width`), and its total integral (zero = nothing to sample).
    pub fn flatten_distribution(&self) -> FlatEnvDist {
        let mut cond_cdf = Vec::with_capacity(self.height * (self.width + 1));
        let mut func = Vec::with_capacity(self.height * self.width);
        for row in &self.dist.conditional {
            cond_cdf.extend_from_slice(&row.cdf);
            func.extend_from_slice(&row.func);
        }
        FlatEnvDist {
            cond_cdf,
            marg_cdf: self.dist.marginal.cdf.clone(),
            func,
            integral: self.dist.marginal.integral,
        }
    }

    /// Bilinear lookup of the raw map at texture coords `(u, v)` in `[0, 1)`.
    fn bilinear(&self, u: Float, v: Float) -> Color {
        let fx = u * self.width as Float - 0.5;
        let fy = v * self.height as Float - 0.5;
        let x0 = fx.floor();
        let y0 = fy.floor();
        let dx = fx - x0;
        let dy = fy - y0;
        let x0 = x0 as isize;
        let y0 = y0 as isize;
        let px = |x: isize, y: isize| -> Color {
            // Wrap horizontally, clamp vertically.
            let xi = x.rem_euclid(self.width as isize) as usize;
            let yi = y.clamp(0, self.height as isize - 1) as usize;
            self.pixels[yi * self.width + xi]
        };
        let c00 = px(x0, y0);
        let c10 = px(x0 + 1, y0);
        let c01 = px(x0, y0 + 1);
        let c11 = px(x0 + 1, y0 + 1);
        let top = c00 * (1.0 - dx) + c10 * dx;
        let bot = c01 * (1.0 - dx) + c11 * dx;
        top * (1.0 - dy) + bot * dy
    }
}

/// GPU-upload-ready view of an [`EnvMap`]'s importance-sampling tables (see
/// [`EnvMap::flatten_distribution`]).
pub struct FlatEnvDist {
    pub cond_cdf: Vec<Float>,
    pub marg_cdf: Vec<Float>,
    pub func: Vec<Float>,
    pub integral: Float,
}

/// Map texture coords `(u, v)` in `[0, 1)` to a unit direction. `v = 0` is the
/// +y pole; `u` wraps azimuth.
#[inline]
fn uv_to_dir(u: Float, v: Float) -> Vec3 {
    let theta = v * PI;
    let phi = u * 2.0 * PI;
    let sin_theta = theta.sin();
    Vec3::new(sin_theta * phi.cos(), theta.cos(), sin_theta * phi.sin())
}

/// Inverse of [`uv_to_dir`].
#[inline]
fn dir_to_uv(dir: Vec3) -> (Float, Float) {
    let theta = dir.y.clamp(-1.0, 1.0).acos();
    let mut phi = dir.z.atan2(dir.x);
    if phi < 0.0 {
        phi += 2.0 * PI;
    }
    (phi / (2.0 * PI), theta / PI)
}

/// Rotate a direction about the +y axis by `angle` radians.
#[inline]
fn rotate_y(d: Vec3, angle: Float) -> Vec3 {
    if angle == 0.0 {
        return d;
    }
    let (s, c) = angle.sin_cos();
    Vec3::new(c * d.x + s * d.z, d.y, -s * d.x + c * d.z)
}

/// Piecewise-constant 1D distribution over `[0, 1)` (pbrt-style).
struct Distribution1D {
    func: Vec<Float>,
    cdf: Vec<Float>,
    integral: Float,
}

impl Distribution1D {
    fn new(func: &[Float]) -> Distribution1D {
        let n = func.len();
        let mut cdf = vec![0.0; n + 1];
        for i in 1..=n {
            cdf[i] = cdf[i - 1] + func[i - 1] / n as Float;
        }
        let integral = cdf[n];
        if integral > 0.0 {
            for c in cdf.iter_mut().skip(1) {
                *c /= integral;
            }
        } else {
            // Degenerate (all zero): use a uniform cdf.
            for (i, c) in cdf.iter_mut().enumerate() {
                *c = i as Float / n as Float;
            }
        }
        Distribution1D {
            func: func.to_vec(),
            cdf,
            integral,
        }
    }

    fn count(&self) -> usize {
        self.func.len()
    }

    /// Sample a continuous value in `[0, 1)`; returns `(x, pdf, offset)`.
    fn sample_continuous(&self, u: Float) -> (Float, Float, usize) {
        let offset = find_interval(&self.cdf, u);
        let mut du = u - self.cdf[offset];
        let span = self.cdf[offset + 1] - self.cdf[offset];
        if span > 0.0 {
            du /= span;
        }
        let pdf = if self.integral > 0.0 {
            self.func[offset] / self.integral
        } else {
            0.0
        };
        let x = (offset as Float + du) / self.count() as Float;
        (x, pdf, offset)
    }
}

/// Piecewise-constant 2D distribution over `[0, 1)²`.
struct Distribution2D {
    conditional: Vec<Distribution1D>,
    marginal: Distribution1D,
}

impl Distribution2D {
    fn new(func: &[Float], nu: usize, nv: usize) -> Distribution2D {
        let conditional: Vec<Distribution1D> = (0..nv)
            .map(|v| Distribution1D::new(&func[v * nu..v * nu + nu]))
            .collect();
        let marginal_func: Vec<Float> = conditional.iter().map(|c| c.integral).collect();
        let marginal = Distribution1D::new(&marginal_func);
        Distribution2D {
            conditional,
            marginal,
        }
    }

    /// Sample `(u, v)` and return it with the joint pdf (in `[0,1)²` measure).
    fn sample_continuous(&self, u1: Float, u2: Float) -> ((Float, Float), Float) {
        let (v, pdf_v, row) = self.marginal.sample_continuous(u2);
        let (u, pdf_u, _) = self.conditional[row].sample_continuous(u1);
        ((u, v), pdf_u * pdf_v)
    }

    /// Joint pdf at `(u, v)`.
    fn pdf(&self, u: Float, v: Float) -> Float {
        let nv = self.conditional.len();
        let iv = ((v * nv as Float) as usize).min(nv - 1);
        // pdf(u|v) * pdf(v) with the integrals canceling to func/marginal.integral
        if self.marginal.integral <= 0.0 {
            return 0.0;
        }
        let nu = self.conditional[iv].count();
        let iu = ((u * nu as Float) as usize).min(nu - 1);
        self.conditional[iv].func[iu] / self.marginal.integral
    }
}

/// Largest `i` with `cdf[i] <= u` (clamped to `[0, len-2]`).
fn find_interval(cdf: &[Float], u: Float) -> usize {
    let mut lo = 0usize;
    let mut hi = cdf.len() - 1;
    while lo + 1 < hi {
        let mid = (lo + hi) / 2;
        if cdf[mid] <= u {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    lo
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dir_uv_round_trip() {
        for d in [
            Vec3::new(0.0, 1.0, 0.0),
            Vec3::new(0.0, -1.0, 0.0),
            Vec3::new(1.0, 0.0, 0.0),
            Vec3::new(-0.3, 0.4, 0.86).normalize(),
        ] {
            let (u, v) = dir_to_uv(d);
            let d2 = uv_to_dir(u, v);
            assert!((d - d2).length() < 1e-3, "round trip failed for {d:?}");
        }
    }

    #[test]
    fn sampling_concentrates_on_bright_region() {
        // A dark map with one bright pixel: samples should land near it and the
        // pdf there should be high.
        let (w, h) = (16, 8);
        let mut pixels = vec![Color::splat(0.001); w * h];
        let bright = (3, 5); // (v, u)
        pixels[bright.0 * w + bright.1] = Color::splat(1000.0);
        let env = EnvMap::from_pixels(w, h, pixels, 1.0, 0.0);

        let mut sampler = Sampler::random(1);
        let mut hits = 0;
        let n = 5000;
        for _ in 0..n {
            let s = env.sample(&mut sampler).unwrap();
            assert!(s.pdf > 0.0);
            let (u, v) = dir_to_uv(s.dir);
            let (iu, iv) = (
                ((u * w as Float) as usize).min(w - 1),
                ((v * h as Float) as usize).min(h - 1),
            );
            if iv == bright.0 && iu == bright.1 {
                hits += 1;
            }
        }
        // The bright pixel is ~1/128 of the map by area but should capture the
        // overwhelming majority of samples.
        assert!(
            hits as f32 / n as f32 > 0.9,
            "only {hits}/{n} hit the bright cell"
        );
    }

    fn varied_env() -> EnvMap {
        let (w, h) = (32, 16);
        let mut pixels = vec![Color::ZERO; w * h];
        for v in 0..h {
            for u in 0..w {
                pixels[v * w + u] = Color::splat(0.1 + (u as Float * 0.13).sin().abs());
            }
        }
        EnvMap::from_pixels(w, h, pixels, 1.0, 0.0)
    }

    #[test]
    fn sampling_pdf_is_normalized() {
        // E[1/pdf] over the sampled set estimates the total solid angle (4π).
        let env = varied_env();
        let mut sampler = Sampler::random(7);
        let n = 50000;
        let mut integral = 0.0;
        for _ in 0..n {
            let s = env.sample(&mut sampler).unwrap();
            assert!(s.pdf > 0.0);
            integral += 1.0 / s.pdf;
        }
        let est = integral / n as Float / (4.0 * PI);
        assert!((est - 1.0).abs() < 0.03, "sampling pdf integral {est} != 1");
    }

    #[test]
    fn pdf_function_is_normalized() {
        // Independently, ∫ pdf(ω) dω = 1: estimate via uniform-sphere sampling,
        // 4π · mean(pdf) ≈ 1. This validates pdf(dir) on its own.
        let env = varied_env();
        let mut sampler = Sampler::random(11);
        let n = 50000;
        let mut sum = 0.0;
        for _ in 0..n {
            let dir = crate::math::sampling::random_unit_vector(&mut sampler);
            sum += env.pdf(dir);
        }
        let est = 4.0 * PI * sum / n as Float;
        assert!((est - 1.0).abs() < 0.03, "pdf(dir) integral {est} != 1");
    }
}
