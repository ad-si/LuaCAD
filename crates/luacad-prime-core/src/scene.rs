//! The renderable scene: a material table, a BVH of primitives, a camera
//! configuration, and a background.

use crate::bvh::Bvh;
use crate::camera::CameraConfig;
use crate::color::luminance;
use crate::env::EnvMap;
use crate::geometry::Primitive;
use crate::hit::HitRecord;
use crate::material::Material;
use crate::math::Vec3;
use crate::ray::Ray;
use crate::sampler::Sampler;
use crate::{Color, Float, MaterialId};

/// What a ray sees when it escapes the scene.
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Copy, Debug)]
pub enum Background {
    /// Constant color in every direction.
    Solid(Color),
    /// Vertical gradient between `bottom` (looking down) and `top` (looking up).
    Gradient { bottom: Color, top: Color },
}

impl Background {
    /// Sample the background along `dir` (the escaping ray direction).
    #[inline]
    pub fn sample(&self, dir: Vec3) -> Color {
        match self {
            Background::Solid(c) => *c,
            Background::Gradient { bottom, top } => {
                let t = 0.5 * (dir.normalize().y + 1.0);
                *bottom * (1.0 - t) + *top * t
            }
        }
    }
}

impl Default for Background {
    fn default() -> Self {
        Background::Gradient {
            bottom: Color::ONE,
            top: Color::new(0.5, 0.7, 1.0),
        }
    }
}

/// An emissive primitive, retained for direct light sampling (NEE).
struct Light {
    prim: Primitive,
    emit: Color,
    area: Float,
    /// Unnormalized selection weight: `luminance(emit) * area` (the light's
    /// emitted power, up to a constant). Lights are picked proportionally to
    /// this, so a dim candle is not sampled as often as a bright ceiling panel.
    weight: Float,
}

/// The result of sampling a light: a direction toward it, the distance to the
/// sampled point, the solid-angle pdf, and the light's emitted radiance.
pub struct LightSample {
    pub wi: Vec3,
    pub dist: Float,
    pub pdf: Float,
    pub emit: Color,
}

pub struct Scene {
    materials: Vec<Material>,
    bvh: Bvh,
    lights: Vec<Light>,
    /// Cumulative normalized light weights, parallel to `lights` (last = 1).
    light_cdf: Vec<Float>,
    /// Whether any material is a dispersive dielectric (the integrator then
    /// samples one wavelength per path — see [`crate::spectrum`]).
    has_dispersion: bool,
    /// Sum of all light weights (for reconstructing selection probabilities).
    light_total_weight: Float,
    env: Option<EnvMap>,
    pub camera: CameraConfig,
    pub background: Background,
}

impl Scene {
    /// Build a scene, constructing the BVH from `primitives` and collecting the
    /// emissive ones into a light list for direct sampling.
    pub fn new(
        materials: Vec<Material>,
        primitives: Vec<Primitive>,
        camera: CameraConfig,
        background: Background,
    ) -> Scene {
        let lights: Vec<Light> = primitives
            .iter()
            .filter_map(|p| {
                let emit = materials[p.material()].emitted();
                let area = p.area();
                let weight = luminance(emit) * area;
                // Zero-power emitters (black emit, degenerate area) are useless
                // NEE targets; leave them to BSDF sampling.
                (weight > 0.0).then_some(Light {
                    prim: *p,
                    emit,
                    area,
                    weight,
                })
            })
            .collect();

        let has_dispersion = materials
            .iter()
            .any(|m| matches!(m, Material::Dielectric { dispersion, .. } if *dispersion > 0.0));

        let light_total_weight: Float = lights.iter().map(|l| l.weight).sum();
        let mut acc = 0.0;
        let mut light_cdf: Vec<Float> = lights
            .iter()
            .map(|l| {
                acc += l.weight / light_total_weight;
                acc
            })
            .collect();
        if let Some(last) = light_cdf.last_mut() {
            *last = 1.0;
        }

        Scene {
            materials,
            bvh: Bvh::build(primitives),
            lights,
            light_cdf,
            has_dispersion,
            light_total_weight,
            env: None,
            camera,
            background,
        }
    }

    /// Attach an environment map (image-based lighting). Replaces the
    /// [`Background`] for escaped rays and is importance-sampled by the
    /// integrator.
    pub fn set_environment(&mut self, env: EnvMap) {
        self.env = Some(env);
    }

    /// The environment map, if one is attached.
    pub fn environment(&self) -> Option<&EnvMap> {
        self.env.as_ref()
    }

    /// Whether the scene contains a dispersive dielectric.
    #[inline]
    pub fn has_dispersion(&self) -> bool {
        self.has_dispersion
    }

    pub fn num_lights(&self) -> usize {
        self.lights.len()
    }

    /// Pick a light proportionally to its power and sample a direction toward
    /// it, returning the solid-angle pdf for connecting the shading point `p`.
    /// Sphere lights sample the cone they subtend (every sample lands on the
    /// visible side); triangle lights sample their area uniformly.
    pub fn sample_light(&self, p: Vec3, sampler: &mut Sampler) -> Option<LightSample> {
        if self.lights.is_empty() {
            return None;
        }
        let u = sampler.next_1d();
        let idx = self
            .light_cdf
            .partition_point(|&c| c <= u)
            .min(self.lights.len() - 1);
        let light = &self.lights[idx];
        let p_select = light.weight / self.light_total_weight;

        // Sphere lights: solid-angle (cone) sampling.
        if let Primitive::Sphere(s) = &light.prim {
            if let Some((wi, dist, pdf)) = s.sample_cone(p, sampler) {
                return Some(LightSample {
                    wi,
                    dist,
                    pdf: pdf * p_select,
                    emit: light.emit,
                });
            }
            // `p` is inside the sphere: fall through to uniform area sampling.
        }

        let (q, n_light) = light.prim.sample(sampler);
        let d = q - p;
        let dist2 = d.length_squared();
        if dist2 < 1e-8 {
            return None;
        }
        let dist = dist2.sqrt();
        let wi = d / dist;
        // Two-sided emitters: use the absolute cosine at the light.
        let cos_light = n_light.dot(wi).abs();
        if cos_light < 1e-6 {
            return None;
        }
        let pdf = dist2 * p_select / (light.area * cos_light);
        Some(LightSample {
            wi,
            dist,
            pdf,
            emit: light.emit,
        })
    }

    /// Solid-angle pdf that [`Scene::sample_light`] would assign to a path ray
    /// (leaving `origin` along `dir`) that landed on the emitter described by
    /// `hit`. Used for the MIS weight when a BSDF-sampled ray hits a light.
    pub fn light_pdf(&self, origin: Vec3, dir: Vec3, hit: &HitRecord) -> Float {
        if self.lights.is_empty() || hit.area <= 0.0 {
            return 0.0;
        }
        // Selection probability, reconstructed from the same quantities the
        // light list was built from (needs no light identity).
        let weight = luminance(self.materials[hit.material].emitted()) * hit.area;
        if weight <= 0.0 {
            return 0.0;
        }
        let p_select = weight / self.light_total_weight;

        // Sphere emitters are cone-sampled (unless `origin` was inside them —
        // then `cone_pdf` is `None` and the area measure below applies, exactly
        // mirroring the sampling-side fallback).
        if let Some(Primitive::Sphere(s)) = self.bvh.primitive(hit.prim as usize) {
            if let Some(pdf) = s.cone_pdf(origin) {
                return pdf * p_select;
            }
        }

        let cos = hit.normal.dot(dir).abs();
        if cos < 1e-6 {
            return 0.0;
        }
        (hit.t * hit.t) * p_select / (hit.area * cos)
    }

    /// Is anything between `origin` and `origin + dir * t_max` (within
    /// `[t_min, t_max]`)? Used for shadow rays.
    pub fn occluded(&self, origin: Vec3, dir: Vec3, t_min: Float, t_max: Float) -> bool {
        self.bvh.occluded(&Ray::new(origin, dir), t_min, t_max)
    }

    #[inline]
    pub fn material(&self, id: MaterialId) -> &Material {
        &self.materials[id]
    }

    #[inline]
    pub fn hit(&self, ray: &Ray, t_min: Float, t_max: Float) -> Option<HitRecord> {
        self.bvh.hit(ray, t_min, t_max)
    }

    /// Flatten the scene's BVH into GPU-upload-ready arrays (see
    /// [`crate::bvh::FlatBvh`]).
    pub fn flatten_bvh(&self) -> crate::bvh::FlatBvh {
        self.bvh.flatten()
    }

    pub fn primitive_count(&self) -> usize {
        self.bvh.len()
    }

    pub fn material_count(&self) -> usize {
        self.materials.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geometry::{Sphere, Triangle};
    use crate::Color;

    fn emissive_scene(prims: Vec<Primitive>, emits: Vec<Color>) -> Scene {
        let materials = emits
            .into_iter()
            .map(|e| Material::Emissive { emit: e })
            .collect();
        Scene::new(
            materials,
            prims,
            CameraConfig::default(),
            Background::Solid(Color::ZERO),
        )
    }

    #[test]
    fn lights_are_picked_proportionally_to_power() {
        // A dim light at -x and a 9x brighter one (same area) at +x: power
        // weighting must pick the bright one ~90% of the time.
        let prims = vec![
            Primitive::Sphere(Sphere::new(Vec3::new(-5.0, 0.0, 0.0), 0.5, 0)),
            Primitive::Sphere(Sphere::new(Vec3::new(5.0, 0.0, 0.0), 0.5, 1)),
        ];
        let scene = emissive_scene(prims, vec![Color::splat(1.0), Color::splat(9.0)]);
        let mut sampler = Sampler::random(1);
        let mut bright = 0u32;
        let n = 4000;
        for _ in 0..n {
            let ls = scene.sample_light(Vec3::ZERO, &mut sampler).unwrap();
            if ls.wi.x > 0.0 {
                bright += 1;
            }
        }
        let frac = bright as f64 / n as f64;
        assert!(
            (frac - 0.9).abs() < 0.03,
            "bright light picked {frac}, want ~0.9"
        );
    }

    #[test]
    fn sphere_light_sample_and_pdf_agree() {
        let prims = vec![Primitive::Sphere(Sphere::new(
            Vec3::new(0.0, 5.0, 0.0),
            1.0,
            0,
        ))];
        let scene = emissive_scene(prims, vec![Color::splat(5.0)]);
        let mut sampler = Sampler::random(2);
        let p = Vec3::ZERO;
        for _ in 0..200 {
            let ls = scene.sample_light(p, &mut sampler).unwrap();
            // Every cone sample must actually reach the sphere...
            let hit = scene
                .hit(&Ray::new(p, ls.wi), 1e-4, Float::INFINITY)
                .expect("cone sample must hit the light");
            assert!(
                (hit.t - ls.dist).abs() < 1e-2,
                "distance mismatch: {} vs {}",
                hit.t,
                ls.dist
            );
            // ...and the reverse pdf must match the sampling pdf (MIS
            // consistency between NEE and BSDF-hits-emitter).
            let pdf = scene.light_pdf(p, ls.wi, &hit);
            assert!(
                ((pdf - ls.pdf) / ls.pdf).abs() < 1e-3,
                "pdf mismatch: {} vs {}",
                pdf,
                ls.pdf
            );
        }
    }

    #[test]
    fn triangle_light_sample_and_pdf_agree() {
        // A quad light overhead, as two triangles.
        let (a, b, c, d) = (
            Vec3::new(-1.0, 3.0, -1.0),
            Vec3::new(1.0, 3.0, -1.0),
            Vec3::new(1.0, 3.0, 1.0),
            Vec3::new(-1.0, 3.0, 1.0),
        );
        let prims = vec![
            Primitive::Triangle(Triangle::new(a, b, c, 0)),
            Primitive::Triangle(Triangle::new(a, c, d, 0)),
        ];
        let scene = emissive_scene(prims, vec![Color::splat(4.0)]);
        let mut sampler = Sampler::random(3);
        let p = Vec3::ZERO;
        let mut checked = 0;
        for _ in 0..200 {
            let Some(ls) = scene.sample_light(p, &mut sampler) else {
                continue;
            };
            let Some(hit) = scene.hit(&Ray::new(p, ls.wi), 1e-4, Float::INFINITY) else {
                continue;
            };
            let pdf = scene.light_pdf(p, ls.wi, &hit);
            assert!(
                ((pdf - ls.pdf) / ls.pdf).abs() < 1e-2,
                "pdf mismatch: {} vs {}",
                pdf,
                ls.pdf
            );
            checked += 1;
        }
        assert!(checked > 150, "too few valid samples: {checked}");
    }

    #[test]
    fn inside_a_sphere_light_falls_back_to_area_sampling() {
        // From inside the sphere there is no cone; both the sampling side and
        // the pdf side must fall back to the (matching) area measure.
        let prims = vec![Primitive::Sphere(Sphere::new(Vec3::ZERO, 5.0, 0))];
        let scene = emissive_scene(prims, vec![Color::splat(2.0)]);
        let mut sampler = Sampler::random(4);
        let p = Vec3::new(1.0, 0.0, 0.0); // inside
        for _ in 0..100 {
            let Some(ls) = scene.sample_light(p, &mut sampler) else {
                continue;
            };
            let hit = scene
                .hit(&Ray::new(p, ls.wi), 1e-4, Float::INFINITY)
                .expect("must hit the enclosing sphere");
            let pdf = scene.light_pdf(p, ls.wi, &hit);
            assert!(
                ((pdf - ls.pdf) / ls.pdf).abs() < 1e-2,
                "pdf mismatch: {} vs {}",
                pdf,
                ls.pdf
            );
        }
    }
}
