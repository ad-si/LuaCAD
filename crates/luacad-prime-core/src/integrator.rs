//! The path-tracing integrator.
//!
//! Compared to the legacy `PathTracer` (recursive, with intertwined and partly
//! buggy direct-illumination logic, and a fresh `ExecutorService` leaked on
//! every render), this integrator is:
//!
//! * **iterative** — radiance and throughput are carried in locals, so depth is
//!   bounded by a loop, not the call stack;
//! * **data-parallel** — rows are rendered concurrently with Rayon over the
//!   global pool, nothing is leaked;
//! * **quasi-Monte-Carlo** — every decision is drawn from a per-pixel-sample
//!   [`Sampler`] (scrambled Halton, see [`crate::sampler`]), so a render is
//!   deterministic *and* converges with much less noise than white noise;
//! * **next-event estimated** — direct light sampling + multiple importance
//!   sampling clean up scenes with area lights.
//!
//! Two front-ends share the same machinery: [`render`] does a one-shot batch
//! render; [`ProgressiveRenderer`] accumulates samples pass-by-pass for
//! interactive viewers (each pass advances the global sample index, so passes
//! continue the low-discrepancy sequence rather than repeating it).

use crate::camera::{Camera, CameraConfig};
use crate::color::{self, Tonemap};
use crate::framebuffer::Framebuffer;
use crate::math::Vec3;
use crate::ray::Ray;
use crate::sampler::Sampler;
use crate::scene::Scene;
use crate::spectrum;
use crate::{Color, Float};
use rayon::prelude::*;

#[derive(Clone, Copy, Debug)]
pub struct RenderSettings {
    pub width: usize,
    pub height: usize,
    pub samples_per_pixel: usize,
    pub max_depth: usize,
    /// Base RNG seed; the same seed reproduces the same image.
    pub seed: u64,
    /// Use the low-discrepancy (quasi-Monte-Carlo) sampler. Disable for plain
    /// white-noise sampling (mostly useful for comparison).
    pub low_discrepancy: bool,
    /// Clamp each path sample's radiance to this maximum to suppress fireflies.
    /// `<= 0` disables clamping (keeping the estimator unbiased).
    pub firefly_clamp: Float,
    pub tonemap: Tonemap,
    pub gamma: Float,
}

impl Default for RenderSettings {
    fn default() -> Self {
        RenderSettings {
            width: 800,
            height: 450,
            samples_per_pixel: 64,
            max_depth: 32,
            seed: 0,
            low_discrepancy: true,
            firefly_clamp: 0.0,
            tonemap: Tonemap::Clamp,
            gamma: 2.2,
        }
    }
}

/// Shadow-ray/self-intersection epsilon (in world units).
const T_MIN: Float = 1e-3;
/// Bounce after which Russian roulette path termination kicks in.
const RR_START_DEPTH: usize = 4;

/// Render `scene` into a fresh framebuffer. `on_row_done` is invoked once per
/// completed scanline (for progress reporting); pass `|| {}` to ignore it.
pub fn render<F>(scene: &Scene, settings: &RenderSettings, on_row_done: F) -> Framebuffer
where
    F: Fn() + Sync + Send,
{
    let mut fb = Framebuffer::new(settings.width, settings.height);
    let camera = Camera::new(
        &scene.camera,
        settings.width as Float / settings.height as Float,
    );

    let width = settings.width;
    let height = settings.height;
    let spp = settings.samples_per_pixel;
    let inv_spp = 1.0 / spp as Float;
    let max_depth = settings.max_depth;
    let clamp = settings.firefly_clamp;
    let seed = settings.seed;
    let qmc = settings.low_discrepancy;

    fb.pixels_mut()
        .par_chunks_mut(width)
        .enumerate()
        .for_each(|(y, row)| {
            for (x, pixel) in row.iter_mut().enumerate() {
                let mut acc = Color::ZERO;
                for k in 0..spp {
                    let mut sampler = if qmc {
                        Sampler::pixel(seed, x, y, k as u32)
                    } else {
                        Sampler::pixel_random(seed, x, y, k as u32)
                    };
                    acc += sample_once(
                        scene,
                        &camera,
                        x,
                        y,
                        width,
                        height,
                        max_depth,
                        clamp,
                        &mut sampler,
                    );
                }
                *pixel = acc * inv_spp;
            }
            on_row_done();
        });

    fb
}

/// Convenience: render and resolve to interleaved RGB8 bytes.
pub fn render_to_srgb<F>(scene: &Scene, settings: &RenderSettings, on_row_done: F) -> Vec<u8>
where
    F: Fn() + Sync + Send,
{
    let fb = render(scene, settings, on_row_done);
    fb.to_srgb_bytes(settings.tonemap, settings.gamma)
}

/// An accumulating renderer for interactive/progressive use.
///
/// Call [`ProgressiveRenderer::render_pass`] repeatedly; each call adds samples
/// to a running per-pixel sum. [`ProgressiveRenderer::to_srgb_bytes`] resolves
/// the current average at any time. To change the camera or resolution, build a
/// new renderer — construction is cheap (it only allocates the accumulation
/// buffer; the scene/BVH is borrowed at render time).
pub struct ProgressiveRenderer {
    width: usize,
    height: usize,
    max_depth: usize,
    seed: u64,
    firefly_clamp: Float,
    camera: Camera,
    sum: Vec<Color>,
    samples: usize,
}

impl ProgressiveRenderer {
    pub fn new(
        camera_config: &CameraConfig,
        width: usize,
        height: usize,
        max_depth: usize,
        seed: u64,
        firefly_clamp: Float,
    ) -> ProgressiveRenderer {
        let camera = Camera::new(camera_config, width as Float / height as Float);
        ProgressiveRenderer {
            width,
            height,
            max_depth,
            seed,
            firefly_clamp,
            camera,
            sum: vec![Color::ZERO; width * height],
            samples: 0,
        }
    }

    pub fn width(&self) -> usize {
        self.width
    }

    pub fn height(&self) -> usize {
        self.height
    }

    /// Total samples-per-pixel accumulated so far.
    pub fn samples(&self) -> usize {
        self.samples
    }

    /// Add `count` more samples per pixel to the accumulation buffer.
    pub fn render_pass(&mut self, scene: &Scene, count: usize) {
        let width = self.width;
        let height = self.height;
        let max_depth = self.max_depth;
        let seed = self.seed;
        let clamp = self.firefly_clamp;
        let base = self.samples;
        let camera = &self.camera;

        self.sum
            .par_chunks_mut(width)
            .enumerate()
            .for_each(|(y, row)| {
                for (x, pixel) in row.iter_mut().enumerate() {
                    let mut acc = Color::ZERO;
                    for local in 0..count {
                        let mut sampler = Sampler::pixel(seed, x, y, (base + local) as u32);
                        acc += sample_once(
                            scene,
                            camera,
                            x,
                            y,
                            width,
                            height,
                            max_depth,
                            clamp,
                            &mut sampler,
                        );
                    }
                    *pixel += acc;
                }
            });

        self.samples += count;
    }

    /// Resolve the current average to interleaved RGB8 bytes.
    pub fn to_srgb_bytes(&self, tonemap: Tonemap, gamma: Float) -> Vec<u8> {
        let inv = if self.samples > 0 {
            1.0 / self.samples as Float
        } else {
            0.0
        };
        let mut out = Vec::with_capacity(self.width * self.height * 3);
        for &c in &self.sum {
            out.extend_from_slice(&color::to_srgb8(c * inv, tonemap, gamma));
        }
        out
    }
}

/// Trace one camera sample for pixel `(x, y)`: QMC dimensions 0–1 jitter the
/// pixel, 2–3 sample the lens (for defocus), and the rest drive the path. The y
/// axis is flipped so row 0 is the top of the image. Applies the firefly clamp.
#[inline]
#[allow(clippy::too_many_arguments)]
fn sample_once(
    scene: &Scene,
    camera: &Camera,
    x: usize,
    y: usize,
    width: usize,
    height: usize,
    max_depth: usize,
    clamp: Float,
    sampler: &mut Sampler,
) -> Color {
    let (du, dv) = sampler.next_2d();
    let s = (x as Float + du) / width as Float;
    let t = (height as Float - 1.0 - y as Float + dv) / height as Float;
    let ray = camera.get_ray(s, t, sampler);
    // Dispersive scenes carry one wavelength per path (see [`crate::spectrum`]):
    // dielectrics refract with its Cauchy index and the RGB contribution is
    // reweighted by the wavelength's responsivity. Everything else renders in
    // plain RGB at the neutral D line (and draws no extra decision, so
    // non-dispersive scenes are bit-identical to before).
    let c = if scene.has_dispersion() {
        let lambda = spectrum::sample_lambda(sampler.next_1d());
        radiance(scene, ray, max_depth, lambda, sampler) * spectrum::rgb_weight(lambda)
    } else {
        radiance(scene, ray, max_depth, spectrum::LAMBDA_D, sampler)
    };
    if clamp > 0.0 {
        c.min(Color::splat(clamp))
    } else {
        c
    }
}

/// Estimate the radiance arriving along `ray` via iterative path tracing with
/// next-event estimation and multiple importance sampling.
fn radiance(
    scene: &Scene,
    mut ray: Ray,
    max_depth: usize,
    lambda: Float,
    sampler: &mut Sampler,
) -> Color {
    let mut l = Color::ZERO;
    let mut throughput = Color::ONE;
    let mut specular_bounce = true;
    let mut prev_bsdf_pdf = 0.0;

    for depth in 0..max_depth {
        let Some(mut hit) = scene.hit(&ray, T_MIN, Float::INFINITY) else {
            // Ray escaped. If an importance-sampled environment is present,
            // MIS-weight its radiance against the env-sampling pdf; otherwise
            // take the (non-sampled) background at full weight.
            if let Some(env) = scene.environment() {
                let le = env.radiance(ray.dir);
                if specular_bounce {
                    l += throughput * le;
                } else {
                    l += throughput * le * power_heuristic(prev_bsdf_pdf, env.pdf(ray.dir));
                }
            } else {
                l += throughput * scene.background.sample(ray.dir);
            }
            break;
        };

        let material = scene.material(hit.material);

        // A segment that ends on the *inside* of a closed surface traveled
        // through that surface's medium: apply Beer–Lambert absorption over
        // its length (a no-op for everything but tinted dielectrics).
        if !hit.front_face {
            throughput = throughput * material.transmittance(hit.t);
        }

        // Apply a normal map (if any) before shading.
        if let Some(nm) = material.normal_map() {
            hit.normal = crate::material::apply_normal_map(&hit, nm);
        }

        // Emission at the hit. If we arrived here by BSDF sampling from a
        // non-specular vertex, MIS-weight it against direct light sampling.
        let emit = material.emitted();
        if emit.max_component() > 0.0 {
            if specular_bounce {
                l += throughput * emit;
            } else {
                let light_pdf = scene.light_pdf(ray.origin, ray.dir, &hit);
                l += throughput * emit * power_heuristic(prev_bsdf_pdf, light_pdf);
            }
            break; // emitters do not scatter
        }

        let wo = -ray.dir;

        // (a) Next-event estimation: connect to a sampled area light. The
        // BSDF value gates the connection (it is zero for directions the
        // material cannot scatter into), and the cosine is taken absolute so
        // transmissive materials (frosted glass) can connect *through* the
        // surface.
        if !material.is_specular() {
            if let Some(ls) = scene.sample_light(hit.p, sampler) {
                if ls.pdf > 0.0 && ls.emit.max_component() > 0.0 {
                    let f = material.eval(wo, ls.wi, &hit, lambda);
                    // Shadow ray, stopping just short of the light surface.
                    if f.max_component() > 0.0
                        && !scene.occluded(hit.p, ls.wi, T_MIN, ls.dist * (1.0 - 1e-3))
                    {
                        let scattering_pdf = material.pdf(wo, ls.wi, &hit, lambda);
                        let w = power_heuristic(ls.pdf, scattering_pdf);
                        let cos_surf = ls.wi.dot(hit.normal).abs();
                        l += throughput * f * ls.emit * (cos_surf * w / ls.pdf);
                    }
                }
            }

            // (a2) Next-event estimation against the environment map.
            if let Some(env) = scene.environment() {
                if let Some(es) = env.sample(sampler) {
                    if es.pdf > 0.0 && es.radiance.max_component() > 0.0 {
                        let f = material.eval(wo, es.dir, &hit, lambda);
                        // The environment is at infinity: test occlusion all the way.
                        if f.max_component() > 0.0
                            && !scene.occluded(hit.p, es.dir, T_MIN, Float::INFINITY)
                        {
                            let scattering_pdf = material.pdf(wo, es.dir, &hit, lambda);
                            let w = power_heuristic(es.pdf, scattering_pdf);
                            let cos_surf = es.dir.dot(hit.normal).abs();
                            l += throughput * f * es.radiance * (cos_surf * w / es.pdf);
                        }
                    }
                }
            }
        }

        // (b) BSDF sampling: extend the path.
        let Some(bs) = material.sample(wo, &hit, lambda, sampler) else {
            break; // absorbed
        };
        if bs.specular {
            throughput = throughput * bs.f;
            specular_bounce = true;
            prev_bsdf_pdf = 0.0;
        } else {
            if bs.pdf <= 0.0 {
                break;
            }
            let cos = bs.wi.dot(hit.normal).abs();
            throughput = throughput * bs.f * (cos / bs.pdf);
            specular_bounce = false;
            prev_bsdf_pdf = bs.pdf;
        }
        ray = Ray::new(hit.p, bs.wi);

        // Russian roulette: unbiasedly terminate dim paths once they have had a
        // chance to gather light.
        if depth >= RR_START_DEPTH {
            let survive = throughput.max_component().clamp(0.05, 0.95);
            if sampler.next_1d() > survive {
                break;
            }
            throughput = throughput / survive;
        }
    }

    l
}

/// Auxiliary feature buffers ("AOVs") for a denoiser, following the
/// conventions Open Image Denoise expects: `albedo` is the surface reflectance
/// seen by the pixel (specular chains are followed, so a mirror or glass pixel
/// carries the albedo of what it shows, tinted by the chain's throughput);
/// `normal` is the shading normal at that same interaction. Rays that escape
/// to the background/environment get white albedo and a zero normal. Both are
/// averaged over the pixel's samples (anti-aliased), not normalized.
pub struct AovBuffers {
    pub albedo: Framebuffer,
    pub normal: Framebuffer,
}

/// Render the denoiser feature buffers for `scene` at `spp` samples per pixel.
/// This is a separate, cheap pass (paths stop at the first non-specular hit),
/// so a small `spp` (8–16) usually suffices; resolution, seed, and depth come
/// from `settings`.
pub fn render_aovs(scene: &Scene, settings: &RenderSettings, spp: usize) -> AovBuffers {
    let width = settings.width;
    let height = settings.height;
    let camera = Camera::new(&scene.camera, width as Float / height as Float);
    let mut albedo = Framebuffer::new(width, height);
    let mut normal = Framebuffer::new(width, height);
    let spp = spp.max(1);
    let inv = 1.0 / spp as Float;
    let seed = settings.seed;
    let qmc = settings.low_discrepancy;
    let max_chain = settings.max_depth;

    albedo
        .pixels_mut()
        .par_chunks_mut(width)
        .zip(normal.pixels_mut().par_chunks_mut(width))
        .enumerate()
        .for_each(|(y, (alb_row, nrm_row))| {
            for x in 0..width {
                let (mut a_acc, mut n_acc) = (Color::ZERO, Color::ZERO);
                for k in 0..spp {
                    let mut sampler = if qmc {
                        Sampler::pixel(seed, x, y, k as u32)
                    } else {
                        Sampler::pixel_random(seed, x, y, k as u32)
                    };
                    let (du, dv) = sampler.next_2d();
                    let s = (x as Float + du) / width as Float;
                    let t = (height as Float - 1.0 - y as Float + dv) / height as Float;
                    let ray = camera.get_ray(s, t, &mut sampler);
                    let (a, n) = aov_once(scene, ray, max_chain, &mut sampler);
                    a_acc += a;
                    n_acc += n;
                }
                alb_row[x] = a_acc * inv;
                nrm_row[x] = n_acc * inv;
            }
        });

    AovBuffers { albedo, normal }
}

/// One AOV sample: walk specular (delta) interactions until the first rough
/// surface, and report its tinted albedo and shading normal.
fn aov_once(scene: &Scene, mut ray: Ray, max_chain: usize, sampler: &mut Sampler) -> (Color, Vec3) {
    let mut tint = Color::ONE;
    for _ in 0..max_chain.max(1) {
        let Some(mut hit) = scene.hit(&ray, T_MIN, Float::INFINITY) else {
            // The environment/background acts as its own "texture": the
            // denoiser should not try to reconstruct detail from geometry
            // that is not there.
            return (tint.min(Color::ONE), Vec3::ZERO);
        };
        let material = scene.material(hit.material);
        if !hit.front_face {
            tint = tint * material.transmittance(hit.t);
        }
        if let Some(nm) = material.normal_map() {
            hit.normal = crate::material::apply_normal_map(&hit, nm);
        }
        if material.is_specular() {
            // Follow the delta chain (mirror reflection, glass refraction) so
            // the pixel's features describe what the surface *shows*.
            let Some(bs) = material.sample(-ray.dir, &hit, spectrum::LAMBDA_D, sampler) else {
                return (tint.min(Color::ONE), hit.normal);
            };
            tint = tint * bs.f;
            ray = Ray::new(hit.p, bs.wi);
            continue;
        }
        let a = tint * material.albedo_hint(hit.u, hit.v);
        return (a.min(Color::ONE), hit.normal);
    }
    // Still inside a specular chain after max_chain bounces: give up cleanly.
    (tint.min(Color::ONE), Vec3::ZERO)
}

/// MIS power heuristic (β = 2) for combining two sampling strategies.
#[inline]
fn power_heuristic(a: Float, b: Float) -> Float {
    let (a2, b2) = (a * a, b * b);
    let s = a2 + b2;
    if s > 0.0 {
        a2 / s
    } else {
        0.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::demo;

    fn mean_luma(bytes: &[u8]) -> f64 {
        let sum: u64 = bytes.iter().map(|&b| b as u64).sum();
        sum as f64 / bytes.len() as f64
    }

    #[test]
    fn progressive_accumulation_advances_and_renders() {
        let scene = demo::cornell_box();
        let mut pr = ProgressiveRenderer::new(&scene.camera, 64, 64, 16, 0, 0.0);
        assert_eq!(pr.samples(), 0);
        pr.render_pass(&scene, 4);
        pr.render_pass(&scene, 4);
        assert_eq!(pr.samples(), 8);
        let bytes = pr.to_srgb_bytes(Tonemap::Clamp, 2.2);
        assert_eq!(bytes.len(), 64 * 64 * 3);
        assert!(mean_luma(&bytes) > 1.0, "image should not be all black");
    }

    #[test]
    fn aovs_report_albedo_and_normal_through_specular_chains() {
        use crate::camera::CameraConfig;
        use crate::geometry::{Primitive, Sphere, Triangle};
        use crate::material::Material;
        use crate::math::Vec3;
        use crate::scene::{Background, Scene};

        // A glass sphere in front of a red wall, camera looking straight at it.
        let red = Color::new(0.8, 0.1, 0.1);
        let materials = vec![
            Material::Dielectric {
                ior: 1.5,
                absorption: Color::ZERO,
                roughness: 0.0,
                dispersion: 0.0,
            },
            Material::Lambertian {
                albedo: red.into(),
                normal: None,
            },
        ];
        let wall = |a, b, c, d, m| {
            [
                Primitive::from(Triangle::new(a, b, c, m)),
                Primitive::from(Triangle::new(a, c, d, m)),
            ]
        };
        let mut prims = vec![Primitive::from(Sphere::new(
            Vec3::new(0.0, 0.0, -3.0),
            1.0,
            0,
        ))];
        prims.extend(wall(
            Vec3::new(-10.0, -10.0, -8.0),
            Vec3::new(10.0, -10.0, -8.0),
            Vec3::new(10.0, 10.0, -8.0),
            Vec3::new(-10.0, 10.0, -8.0),
            1,
        ));
        let camera = CameraConfig {
            look_at: Vec3::new(0.0, 0.0, -3.0),
            ..CameraConfig::default()
        };
        let scene = Scene::new(materials, prims, camera, Background::Solid(Color::ONE));

        let settings = RenderSettings {
            width: 17,
            height: 17,
            max_depth: 16,
            ..RenderSettings::default()
        };
        let aov = render_aovs(&scene, &settings, 32);

        // Center pixel: the ray passes through the glass and lands on the red
        // wall — its albedo must be red-dominated (a few Fresnel-reflected
        // samples see the white background instead).
        let a = aov.albedo.pixel(8, 8);
        assert!(
            a.x > 0.5 && a.y < 0.35,
            "expected red-ish albedo, got {a:?}"
        );

        // A corner pixel sees the red wall directly: albedo ~= red exactly and
        // the normal faces the camera (+z).
        let c = aov.albedo.pixel(0, 0);
        assert!((c - red).length() < 1e-3, "expected wall albedo, got {c:?}");
        let n = aov.normal.pixel(0, 0);
        assert!(n.z > 0.99, "wall normal should face the camera, got {n:?}");

        // Nothing escapes this scene except through Fresnel reflection at the
        // center; the wall fills the frame, so albedo stays in [0, 1].
        for &px in aov.albedo.pixels() {
            assert!(px.max_component() <= 1.0 + 1e-6);
        }
    }

    #[test]
    fn aovs_background_is_white_albedo_zero_normal() {
        use crate::camera::CameraConfig;
        use crate::geometry::{Primitive, Sphere};
        use crate::material::Material;
        use crate::math::Vec3;
        use crate::scene::{Background, Scene};

        // One tiny off-screen sphere; every camera ray escapes.
        let scene = Scene::new(
            vec![Material::Lambertian {
                albedo: Color::ONE.into(),
                normal: None,
            }],
            vec![Primitive::from(Sphere::new(
                Vec3::new(100.0, 0.0, 0.0),
                0.1,
                0,
            ))],
            CameraConfig::default(),
            Background::default(),
        );
        let settings = RenderSettings {
            width: 8,
            height: 8,
            ..RenderSettings::default()
        };
        let aov = render_aovs(&scene, &settings, 4);
        assert_eq!(aov.albedo.pixel(4, 4), Color::ONE);
        assert_eq!(aov.normal.pixel(4, 4), Color::ZERO);
    }

    #[test]
    fn tinted_glass_absorbs_only_the_tinted_channels() {
        use crate::camera::CameraConfig;
        use crate::geometry::{Primitive, Sphere};
        use crate::material::Material;
        use crate::math::Vec3;
        use crate::scene::{Background, Scene};

        // Camera looks through a glass sphere at a white background. Absorption
        // does not change any sampling decision, so with the same seed the path
        // trees are identical and only the throughput differs: the untinted
        // channel must match the clear render exactly, the tinted ones must
        // darken dramatically.
        let build = |absorption: Color| {
            let materials = vec![Material::Dielectric {
                ior: 1.5,
                absorption,
                roughness: 0.0,
                dispersion: 0.0,
            }];
            let prims = vec![Primitive::Sphere(Sphere::new(
                Vec3::new(0.0, 0.0, -3.0),
                1.0,
                0,
            ))];
            let camera = CameraConfig {
                look_at: Vec3::new(0.0, 0.0, -3.0),
                ..CameraConfig::default()
            };
            Scene::new(materials, prims, camera, Background::Solid(Color::ONE))
        };
        let settings = RenderSettings {
            width: 16,
            height: 16,
            samples_per_pixel: 16,
            max_depth: 16,
            ..RenderSettings::default()
        };
        let clear = render(&build(Color::ZERO), &settings, || {});
        let tinted = render(&build(Color::new(2.0, 2.0, 0.0)), &settings, || {});
        let (c, t) = (clear.pixel(8, 8), tinted.pixel(8, 8));
        assert!(c.x > 0.5, "clear glass should pass the white background");
        assert!((t.z - c.z).abs() < 1e-6, "untinted channel must not change");
        assert!(
            t.x < c.x * 0.2,
            "tinted channel must darken: {} vs {}",
            t.x,
            c.x
        );
        assert!(
            t.y < c.y * 0.2,
            "tinted channel must darken: {} vs {}",
            t.y,
            c.y
        );
    }

    #[test]
    fn progressive_matches_batch_within_noise() {
        // Progressive accumulation and a batch render at the same total spp
        // should converge to roughly the same image.
        let scene = demo::spheres();
        let settings = RenderSettings {
            width: 48,
            height: 32,
            samples_per_pixel: 32,
            max_depth: 8,
            seed: 0,
            low_discrepancy: true,
            firefly_clamp: 0.0,
            tonemap: Tonemap::Clamp,
            gamma: 2.2,
        };
        let batch = render_to_srgb(&scene, &settings, || {});

        let mut pr = ProgressiveRenderer::new(&scene.camera, 48, 32, 8, 0, 0.0);
        for _ in 0..32 {
            pr.render_pass(&scene, 1);
        }
        let prog = pr.to_srgb_bytes(Tonemap::Clamp, 2.2);

        let d = (mean_luma(&batch) - mean_luma(&prog)).abs();
        assert!(d < 6.0, "mean luma differs too much: {d}");
    }
}
