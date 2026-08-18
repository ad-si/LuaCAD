//! Surface scattering models (BSDFs).
//!
//! The legacy design had an abstract `Material` base with `IdealDiffuseModel` /
//! `IdealSpecularModel` subclasses, a never-implemented transmission path, an
//! unused `MicrofacetDistribution` stub, and dead branches in the specular
//! BRDF. Here the closed set of models is a single `enum` — a true sealed
//! hierarchy the compiler exhaustively checks and dispatches without virtual
//! calls — dielectric transmission is implemented, and the microfacet stub is
//! realized as a GGX conductor.
//!
//! Each model exposes the three operations a multiple-importance-sampling
//! integrator needs:
//!
//! * [`Material::sample`] — importance-sample an outgoing direction;
//! * [`Material::eval`] — evaluate the BSDF `f(wo, wi)`;
//! * [`Material::pdf`] — the solid-angle pdf that `sample` would assign to `wi`.
//!
//! Direction convention: `wo` points from the surface toward the viewer
//! (`-ray.dir`) and `wi` points from the surface toward the light / next
//! vertex. The shading normal `hit.normal` is always oriented to face `wo`.

use crate::hit::HitRecord;
use crate::math::sampling::cosine_weighted_hemisphere;
use crate::math::{Onb, Vec3};
use crate::sampler::Sampler;
use crate::texture::{ImageData, Texture};
use crate::{Color, Float};
use std::f32::consts::{FRAC_1_PI, PI};
use std::path::Path;

/// Below this roughness a [`Material::Metal`] is treated as a perfect mirror
/// (a delta BSDF), avoiding the numerical spike of a near-singular GGX lobe.
const MIRROR_ROUGHNESS: Float = 0.02;

#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[derive(Clone, Debug)]
pub enum Material {
    /// Ideal diffuse reflector (cosine-weighted importance sampled).
    Lambertian {
        albedo: Texture,
        /// Optional tangent-space normal map (use `srgb: false` for these).
        #[cfg_attr(feature = "serde", serde(default))]
        normal: Option<Texture>,
    },
    /// GGX microfacet conductor. `roughness` in `[0, 1]`: 0 is a perfect
    /// mirror, higher values are rougher (more blurred reflections).
    Metal {
        albedo: Texture,
        roughness: Float,
        #[cfg_attr(feature = "serde", serde(default))]
        normal: Option<Texture>,
    },
    /// Dielectric (glass/water) with refraction + Fresnel reflection. Smooth
    /// (a delta BSDF) at `roughness` ≤ [`MIRROR_ROUGHNESS`]; above that it is
    /// a GGX microfacet surface with reflection *and* refraction lobes
    /// (Walter et al. 2007) — frosted glass.
    Dielectric {
        ior: Float,
        /// Beer–Lambert absorption coefficient of the interior, per world unit
        /// (higher = denser tint; zero = clear). Light traveling a distance `d`
        /// inside is attenuated by `exp(-absorption * d)`, so the *complement*
        /// of the desired tint absorbs: green glass absorbs red and blue.
        #[cfg_attr(feature = "serde", serde(default))]
        absorption: Color,
        /// Microfacet roughness in `[0, 1]`: 0 is polished glass, higher
        /// values are frostier (blurred reflection and transmission).
        #[cfg_attr(feature = "serde", serde(default))]
        roughness: Float,
        /// Cauchy dispersion coefficient in µm² (`ior` is anchored at the
        /// sodium D line). 0 disables dispersion; BK7 glass is ≈ 0.0042;
        /// exaggerated values (0.02–0.05) throw visible rainbows. See
        /// [`crate::spectrum`].
        #[cfg_attr(feature = "serde", serde(default))]
        dispersion: Float,
    },
    /// Diffuse base under an untinted GGX specular coat — a glossy plastic.
    /// (LuaCAD addition, not in upstream Prime.) `specular` is the coat's
    /// normal-incidence reflectance F0 (≈ 0.04 for typical plastic). The coat
    /// roughness is floored above [`MIRROR_ROUGHNESS`], so this material is
    /// never a delta BSDF and always supports direct light sampling.
    Plastic {
        albedo: Texture,
        /// GGX roughness of the specular coat.
        roughness: Float,
        /// Normal-incidence reflectance (F0) of the coat.
        specular: Float,
        #[cfg_attr(feature = "serde", serde(default))]
        normal: Option<Texture>,
    },
    /// Light source: emits radiance, does not scatter.
    Emissive { emit: Color },
}

/// A sampled scattering direction with its BSDF value and pdf.
pub struct BsdfSample {
    /// Sampled direction (unit, world space).
    pub wi: Vec3,
    /// BSDF value `f(wo, wi)`. For a specular sample this is the full throughput
    /// weight (no cosine/pdf factors are applied by the caller).
    pub f: Color,
    /// Solid-angle pdf of `wi`. Meaningless (and ignored) when `specular`.
    pub pdf: Float,
    /// True for delta BSDFs (perfect mirror / glass): no light sampling applies,
    /// and the caller multiplies throughput by `f` directly.
    pub specular: bool,
}

impl Material {
    /// Radiance emitted by this surface (zero for non-emitters). Emitters are
    /// treated as two-sided.
    #[inline]
    pub fn emitted(&self) -> Color {
        match self {
            Material::Emissive { emit } => *emit,
            _ => Color::ZERO,
        }
    }

    /// Whether this is a delta (perfectly specular) BSDF, which cannot be
    /// connected to via direct light sampling.
    #[inline]
    pub fn is_specular(&self) -> bool {
        match self {
            Material::Metal { roughness, .. } | Material::Dielectric { roughness, .. } => {
                *roughness <= MIRROR_ROUGHNESS
            }
            _ => false,
        }
    }

    /// Transmittance of a straight-line segment of length `distance` through
    /// this material's interior (Beer–Lambert). `ONE` for anything that is not
    /// an absorbing dielectric. The integrator applies this to the throughput
    /// whenever a path segment ends on the *inside* of a surface — for closed
    /// geometry that segment necessarily lay within the medium.
    #[inline]
    pub fn transmittance(&self, distance: Float) -> Color {
        match self {
            Material::Dielectric { absorption, .. } if absorption.max_component() > 0.0 => {
                Color::new(
                    (-absorption.x * distance).exp(),
                    (-absorption.y * distance).exp(),
                    (-absorption.z * distance).exp(),
                )
            }
            _ => Color::ONE,
        }
    }

    /// The surface's reflectance color for denoiser feature buffers (the
    /// "albedo" AOV): the albedo texture for Lambertian/Metal, white for
    /// emitters and clear dielectrics (whose color comes from what they show,
    /// not from the surface itself).
    #[inline]
    pub fn albedo_hint(&self, u: Float, v: Float) -> Color {
        match self {
            Material::Lambertian { albedo, .. }
            | Material::Metal { albedo, .. }
            | Material::Plastic { albedo, .. } => albedo.sample(u, v),
            _ => Color::ONE,
        }
    }

    /// The material's tangent-space normal map, if any.
    #[inline]
    pub fn normal_map(&self) -> Option<&Texture> {
        match self {
            Material::Lambertian { normal, .. }
            | Material::Metal { normal, .. }
            | Material::Plastic { normal, .. } => normal.as_ref(),
            _ => None,
        }
    }

    /// Importance-sample an outgoing direction, or `None` if absorbed.
    pub fn sample(
        &self,
        wo: Vec3,
        hit: &HitRecord,
        lambda: Float,
        sampler: &mut Sampler,
    ) -> Option<BsdfSample> {
        let n = hit.normal;
        match self {
            Material::Lambertian { albedo, .. } => {
                let mut wi = cosine_weighted_hemisphere(sampler, n);
                if wi.is_near_zero() {
                    wi = n;
                }
                let wi = wi.normalize();
                let cos = wi.dot(n);
                if cos <= 0.0 {
                    return None;
                }
                Some(BsdfSample {
                    wi,
                    f: albedo.sample(hit.u, hit.v) * FRAC_1_PI,
                    pdf: cos * FRAC_1_PI,
                    specular: false,
                })
            }

            Material::Metal {
                albedo, roughness, ..
            } => {
                let roughness = *roughness;
                if roughness <= MIRROR_ROUGHNESS {
                    // Perfect mirror: a delta BSDF.
                    let wi = (-wo).reflect(n);
                    if wi.dot(n) <= 0.0 {
                        return None;
                    }
                    return Some(BsdfSample {
                        wi: wi.normalize(),
                        f: albedo.sample(hit.u, hit.v),
                        pdf: 1.0,
                        specular: true,
                    });
                }
                // Rough conductor: sample a microfacet normal via the GGX VNDF,
                // reflect about it, then reuse eval()/pdf() so the three stay
                // consistent.
                let a = ggx_alpha(roughness);
                let onb = Onb::from_w(n);
                let wo_l = Vec3::new(wo.dot(onb.u), wo.dot(onb.v), wo.dot(onb.w));
                if wo_l.z <= 0.0 {
                    return None;
                }
                let h = onb.local(sample_ggx_vndf(wo_l, a, sampler));
                let wi = (-wo).reflect(h);
                if wi.dot(n) <= 0.0 {
                    return None;
                }
                let pdf = self.pdf(wo, wi, hit, lambda);
                if pdf <= 0.0 {
                    return None;
                }
                Some(BsdfSample {
                    wi,
                    f: self.eval(wo, wi, hit, lambda),
                    pdf,
                    specular: false,
                })
            }

            Material::Dielectric {
                ior,
                roughness,
                dispersion,
                ..
            } => {
                let ior = crate::spectrum::cauchy_ior(*ior, *dispersion, lambda);
                let roughness = *roughness;
                // Ratio of indices, incident side over transmitted side.
                let eta_ratio = if hit.front_face { 1.0 / ior } else { ior };

                if roughness <= MIRROR_ROUGHNESS {
                    // Polished glass: a delta BSDF about the geometric normal.
                    let incoming = -wo; // == ray.dir
                    let cos_theta = wo.dot(n).min(1.0);
                    let sin_theta = (1.0 - cos_theta * cos_theta).max(0.0).sqrt();
                    let cannot_refract = eta_ratio * sin_theta > 1.0;
                    let wi = if cannot_refract
                        || schlick_reflectance(cos_theta, eta_ratio) > sampler.next_1d()
                    {
                        incoming.reflect(n)
                    } else {
                        incoming.refract(n, eta_ratio)
                    };
                    return Some(BsdfSample {
                        wi: wi.normalize(),
                        f: Color::ONE,
                        pdf: 1.0,
                        specular: true,
                    });
                }

                // Frosted glass (Walter 2007): sample a microfacet normal from
                // the GGX VNDF, then reflect or refract about *it*, choosing
                // the lobe by its Fresnel weight. eval()/pdf() are reused so
                // the three stay consistent under MIS.
                let a = ggx_alpha(roughness);
                let onb = Onb::from_w(n);
                let wo_l = Vec3::new(wo.dot(onb.u), wo.dot(onb.v), wo.dot(onb.w));
                if wo_l.z <= 0.0 {
                    return None;
                }
                let h = onb.local(sample_ggx_vndf(wo_l, a, sampler));
                let cos_oh = wo.dot(h);
                if cos_oh <= 0.0 {
                    return None;
                }
                let sin2_t = eta_ratio * eta_ratio * (1.0 - cos_oh * cos_oh);
                let fr = if sin2_t > 1.0 {
                    1.0 // total internal reflection at this microfacet
                } else {
                    schlick_reflectance(cos_oh, eta_ratio)
                };
                let wi = if fr > sampler.next_1d() {
                    let wi = (-wo).reflect(h);
                    if wi.dot(n) <= 0.0 {
                        return None; // reflected below the macro-surface
                    }
                    wi.normalize()
                } else {
                    let wi = (-wo).refract(h, eta_ratio);
                    if wi.dot(n) >= 0.0 {
                        return None; // refracted into the wrong hemisphere
                    }
                    wi.normalize()
                };
                let pdf = self.pdf(wo, wi, hit, lambda);
                if pdf <= 0.0 {
                    return None;
                }
                Some(BsdfSample {
                    wi,
                    f: self.eval(wo, wi, hit, lambda),
                    pdf,
                    specular: false,
                })
            }

            Material::Plastic {
                roughness,
                specular,
                ..
            } => {
                let no = wo.dot(n);
                if no <= 0.0 {
                    return None;
                }
                // Pick the coat with its view-angle Fresnel probability so
                // bright grazing highlights are importance-sampled; otherwise
                // sample the diffuse base. eval()/pdf() are reused so the
                // three stay consistent under MIS.
                let q = plastic_coat_weight(no, *specular);
                let wi = if sampler.next_1d() < q {
                    let a = ggx_alpha(plastic_roughness(*roughness));
                    let onb = Onb::from_w(n);
                    let wo_l = Vec3::new(wo.dot(onb.u), wo.dot(onb.v), wo.dot(onb.w));
                    let h = onb.local(sample_ggx_vndf(wo_l, a, sampler));
                    (-wo).reflect(h)
                } else {
                    cosine_weighted_hemisphere(sampler, n)
                };
                if wi.is_near_zero() || wi.dot(n) <= 0.0 {
                    return None;
                }
                let wi = wi.normalize();
                let pdf = self.pdf(wo, wi, hit, lambda);
                if pdf <= 0.0 {
                    return None;
                }
                Some(BsdfSample {
                    wi,
                    f: self.eval(wo, wi, hit, lambda),
                    pdf,
                    specular: false,
                })
            }

            Material::Emissive { .. } => None,
        }
    }

    /// Evaluate the BSDF `f(wo, wi)`. Zero for specular materials (their
    /// contribution is a delta handled by [`Material::sample`]).
    pub fn eval(&self, wo: Vec3, wi: Vec3, hit: &HitRecord, lambda: Float) -> Color {
        let n = hit.normal;
        match self {
            Material::Lambertian { albedo, .. } => {
                if wi.dot(n) > 0.0 && wo.dot(n) > 0.0 {
                    albedo.sample(hit.u, hit.v) * FRAC_1_PI
                } else {
                    Color::ZERO
                }
            }
            Material::Metal {
                albedo, roughness, ..
            } => {
                let roughness = *roughness;
                if roughness <= MIRROR_ROUGHNESS {
                    return Color::ZERO;
                }
                let no = wo.dot(n);
                let nl = wi.dot(n);
                if no <= 0.0 || nl <= 0.0 {
                    return Color::ZERO;
                }
                let h = (wo + wi).normalize();
                let nh = n.dot(h).max(0.0);
                let vh = wo.dot(h).max(0.0);
                let a2 = ggx_alpha(roughness).powi(2);
                let d = ggx_d(nh, a2);
                let g = smith_g2(no, nl, a2);
                let fr = fresnel_schlick(vh, albedo.sample(hit.u, hit.v));
                fr * (d * g / (4.0 * no * nl))
            }
            Material::Dielectric {
                ior,
                roughness,
                dispersion,
                ..
            } => {
                let roughness = *roughness;
                if roughness <= MIRROR_ROUGHNESS {
                    return Color::ZERO;
                }
                let ior = crate::spectrum::cauchy_ior(*ior, *dispersion, lambda);
                match rough_dielectric_lobe(ior, roughness, n, wo, wi, hit.front_face) {
                    Some(lobe) => Color::splat(lobe.f),
                    None => Color::ZERO,
                }
            }
            Material::Plastic {
                albedo,
                roughness,
                specular,
                ..
            } => {
                let no = wo.dot(n);
                let nl = wi.dot(n);
                if no <= 0.0 || nl <= 0.0 {
                    return Color::ZERO;
                }
                let h = (wo + wi).normalize();
                let nh = n.dot(h).max(0.0);
                let vh = wo.dot(h).max(0.0);
                let a2 = ggx_alpha(plastic_roughness(*roughness)).powi(2);
                let m5 = (1.0 - vh).clamp(0.0, 1.0).powi(5);
                let fr = *specular + (1.0 - *specular) * m5;
                let spec = fr * ggx_d(nh, a2) * smith_g2(no, nl, a2) / (4.0 * no * nl);
                // The energy the coat reflects never reaches the base.
                let diffuse = albedo.sample(hit.u, hit.v) * ((1.0 - fr) * FRAC_1_PI);
                diffuse + Color::splat(spec)
            }
            _ => Color::ZERO,
        }
    }

    /// Solid-angle pdf that [`Material::sample`] would assign to `wi`. Zero for
    /// specular materials.
    pub fn pdf(&self, wo: Vec3, wi: Vec3, hit: &HitRecord, lambda: Float) -> Float {
        let n = hit.normal;
        match self {
            Material::Lambertian { .. } => {
                let cos = wi.dot(n);
                if cos > 0.0 {
                    cos * FRAC_1_PI
                } else {
                    0.0
                }
            }
            Material::Metal { roughness, .. } => {
                let roughness = *roughness;
                if roughness <= MIRROR_ROUGHNESS {
                    return 0.0;
                }
                let no = wo.dot(n);
                let nl = wi.dot(n);
                if no <= 0.0 || nl <= 0.0 {
                    return 0.0;
                }
                let h = (wo + wi).normalize();
                let nh = n.dot(h).max(0.0);
                let a2 = ggx_alpha(roughness).powi(2);
                // VNDF pdf: D(h) * G1(wo) / (4 * NdotV).
                ggx_d(nh, a2) * smith_g1(no, a2) / (4.0 * no)
            }
            Material::Dielectric {
                ior,
                roughness,
                dispersion,
                ..
            } => {
                let roughness = *roughness;
                if roughness <= MIRROR_ROUGHNESS {
                    return 0.0;
                }
                let ior = crate::spectrum::cauchy_ior(*ior, *dispersion, lambda);
                match rough_dielectric_lobe(ior, roughness, n, wo, wi, hit.front_face) {
                    Some(lobe) => lobe.pdf,
                    None => 0.0,
                }
            }
            Material::Plastic {
                roughness,
                specular,
                ..
            } => {
                let no = wo.dot(n);
                let nl = wi.dot(n);
                if no <= 0.0 || nl <= 0.0 {
                    return 0.0;
                }
                let h = (wo + wi).normalize();
                let nh = n.dot(h).max(0.0);
                let a2 = ggx_alpha(plastic_roughness(*roughness)).powi(2);
                let q = plastic_coat_weight(no, *specular);
                // Coat: VNDF pdf D(h) * G1(wo) / (4 * NdotV); base: cosine.
                let spec_pdf = ggx_d(nh, a2) * smith_g1(no, a2) / (4.0 * no);
                q * spec_pdf + (1.0 - q) * nl * FRAC_1_PI
            }
            _ => 0.0,
        }
    }

    /// Resolve any image textures (via a front-end decoder) relative to
    /// `base_dir`. No-op for procedural/constant parameters.
    pub fn resolve_textures<F>(&mut self, base_dir: &Path, decoder: &mut F) -> Result<(), String>
    where
        F: FnMut(&Path) -> Result<ImageData, String>,
    {
        let (albedo, normal) = match self {
            Material::Lambertian { albedo, normal } => (Some(albedo), normal.as_mut()),
            Material::Metal { albedo, normal, .. } => (Some(albedo), normal.as_mut()),
            Material::Plastic { albedo, normal, .. } => (Some(albedo), normal.as_mut()),
            _ => (None, None),
        };
        if let Some(a) = albedo {
            a.resolve(base_dir, decoder)?;
        }
        if let Some(n) = normal {
            n.resolve(base_dir, decoder)?;
        }
        Ok(())
    }
}

/// Perturb the shading normal at `hit` using a tangent-space normal map.
///
/// The map encodes a unit normal in `[0, 1]³` (so it should be loaded *linear*,
/// i.e. `srgb: false`). We build a TBN frame from the hit's normal and tangent
/// and rotate the decoded normal into world space.
pub fn apply_normal_map(hit: &HitRecord, normal_map: &Texture) -> Vec3 {
    let n = hit.normal;
    // Orthonormalize the tangent against n (Gram-Schmidt).
    let t = (hit.tangent - n * hit.tangent.dot(n)).normalize_or(crate::hit::fallback_tangent(n));
    let b = n.cross(t);
    let c = normal_map.sample(hit.u, hit.v);
    let m = Vec3::new(c.x * 2.0 - 1.0, c.y * 2.0 - 1.0, c.z * 2.0 - 1.0);
    (t * m.x + b * m.y + n * m.z).normalize_or(n)
}

// --- GGX microfacet helpers -------------------------------------------------

/// Map user roughness to the GGX `α` width parameter (clamped away from the
/// singular mirror limit).
#[inline]
fn ggx_alpha(roughness: Float) -> Float {
    roughness.clamp(1e-3, 1.0)
}

/// The plastic coat never goes below this roughness, so [`Material::Plastic`]
/// is never a delta BSDF and always supports direct light sampling.
#[inline]
fn plastic_roughness(roughness: Float) -> Float {
    roughness.max(2.0 * MIRROR_ROUGHNESS)
}

/// Probability of sampling the plastic's specular coat: its Schlick Fresnel
/// at the view angle, bounded away from 0 and 1 so neither lobe is starved.
#[inline]
fn plastic_coat_weight(n_dot_o: Float, f0: Float) -> Float {
    let m = (1.0 - n_dot_o).clamp(0.0, 1.0);
    let m5 = m * m * m * m * m;
    (f0 + (1.0 - f0) * m5).clamp(0.05, 0.95)
}

/// GGX (Trowbridge-Reitz) normal distribution, `a2 = α²`.
#[inline]
fn ggx_d(n_dot_h: Float, a2: Float) -> Float {
    let x = n_dot_h * n_dot_h * (a2 - 1.0) + 1.0;
    a2 / (PI * x * x)
}

/// Smith masking-shadowing Λ term for the GGX distribution.
#[inline]
fn smith_lambda(cos: Float, a2: Float) -> Float {
    let c2 = (cos * cos).max(1e-8);
    let tan2 = (1.0 - c2) / c2;
    0.5 * (-1.0 + (1.0 + a2 * tan2).sqrt())
}

#[inline]
fn smith_g1(cos: Float, a2: Float) -> Float {
    1.0 / (1.0 + smith_lambda(cos, a2))
}

/// Height-correlated Smith masking-shadowing for a (wo, wi) pair.
#[inline]
fn smith_g2(cos_o: Float, cos_l: Float, a2: Float) -> Float {
    1.0 / (1.0 + smith_lambda(cos_o, a2) + smith_lambda(cos_l, a2))
}

/// Schlick Fresnel for a conductor whose normal-incidence reflectance is `f0`.
#[inline]
fn fresnel_schlick(cos: Float, f0: Color) -> Color {
    let m = (1.0 - cos).clamp(0.0, 1.0);
    let m5 = m * m * m * m * m;
    f0 + (Color::ONE - f0) * m5
}

/// Sample the GGX distribution of visible normals (Heitz 2018), isotropic.
/// `ve` is the view direction in the local frame (z = surface normal); returns
/// a microfacet normal in the same local frame.
fn sample_ggx_vndf(ve: Vec3, alpha: Float, sampler: &mut Sampler) -> Vec3 {
    // Transform the view direction to the hemisphere configuration.
    let vh = Vec3::new(alpha * ve.x, alpha * ve.y, ve.z).normalize();
    // Orthonormal basis around vh.
    let lensq = vh.x * vh.x + vh.y * vh.y;
    let t1 = if lensq > 1e-12 {
        Vec3::new(-vh.y, vh.x, 0.0) * (1.0 / lensq.sqrt())
    } else {
        Vec3::new(1.0, 0.0, 0.0)
    };
    let t2 = vh.cross(t1);
    // Sample a point on the projected disk.
    let (u1, u2) = sampler.next_2d();
    let r = u1.sqrt();
    let phi = 2.0 * PI * u2;
    let p1 = r * phi.cos();
    let mut p2 = r * phi.sin();
    let s = 0.5 * (1.0 + vh.z);
    p2 = (1.0 - s) * (1.0 - p1 * p1).max(0.0).sqrt() + s * p2;
    let pz = (1.0 - p1 * p1 - p2 * p2).max(0.0).sqrt();
    let nh = t1 * p1 + t2 * p2 + vh * pz;
    // Transform back to the ellipsoid configuration.
    Vec3::new(alpha * nh.x, alpha * nh.y, nh.z.max(0.0)).normalize()
}

/// Schlick's polynomial approximation of the Fresnel reflectance (dielectric).
#[inline]
fn schlick_reflectance(cosine: Float, eta_ratio: Float) -> Float {
    let r0 = ((1.0 - eta_ratio) / (1.0 + eta_ratio)).powi(2);
    r0 + (1.0 - r0) * (1.0 - cosine).powi(5)
}

/// Fresnel reflectance of a dielectric facet: Schlick plus an explicit
/// total-internal-reflection check (Schlick alone misses TIR when leaving a
/// denser medium). `cos_i` is measured against the (micro)facet normal;
/// `eta_ratio` is incident-over-transmitted index.
#[inline]
fn dielectric_fresnel(cos_i: Float, eta_ratio: Float) -> Float {
    let sin2_t = eta_ratio * eta_ratio * (1.0 - cos_i * cos_i);
    if sin2_t > 1.0 {
        1.0
    } else {
        schlick_reflectance(cos_i, eta_ratio)
    }
}

/// One lobe of the rough-dielectric BSDF and its sampling pdf, evaluated for a
/// concrete `(wo, wi)` pair (Walter et al. 2007, "Microfacet Models for
/// Refraction through Rough Surfaces"). The hemisphere of `wi` selects the
/// lobe: reflection above the macro-surface, refraction below. `None` where
/// the configuration is geometrically impossible (the BSDF is zero).
///
/// Conventions: `n` is the shading normal oriented toward `wo`; the pdf
/// includes the Fresnel lobe-selection probability, mirroring how
/// [`Material::sample`] picks the lobe — so `sample`/`eval`/`pdf` agree and
/// the sampled throughput weight reduces to `G2/G1 ≤ 1`.
struct DielectricLobe {
    f: Float,
    pdf: Float,
}

fn rough_dielectric_lobe(
    ior: Float,
    roughness: Float,
    n: Vec3,
    wo: Vec3,
    wi: Vec3,
    front_face: bool,
) -> Option<DielectricLobe> {
    let eta_ratio = if front_face { 1.0 / ior } else { ior }; // incident / transmitted
    let rel_eta = 1.0 / eta_ratio; // transmitted / incident
    let no = wo.dot(n);
    let ni = wi.dot(n);
    if no <= 0.0 || ni == 0.0 {
        return None;
    }
    let a2 = ggx_alpha(roughness).powi(2);

    if ni > 0.0 {
        // Reflection lobe (the standard microfacet BRDF, dielectric Fresnel).
        let h = (wo + wi).normalize_or(n);
        let oh = wo.dot(h);
        let nh = n.dot(h);
        if oh <= 0.0 || nh <= 0.0 {
            return None;
        }
        let fr = dielectric_fresnel(oh, eta_ratio);
        let d = ggx_d(nh, a2);
        let f = fr * d * smith_g2(no, ni, a2) / (4.0 * no * ni);
        let pdf = fr * d * smith_g1(no, a2) / (4.0 * no);
        Some(DielectricLobe { f, pdf })
    } else {
        // Transmission lobe. The half-vector is fixed by Snell's law (Walter
        // eq. 16, with the incident index folded into `rel_eta`).
        let ni_abs = -ni;
        let mut h = -(wo + wi * rel_eta);
        if h.length_squared() < 1e-12 {
            return None;
        }
        h = h.normalize();
        if h.dot(n) < 0.0 {
            h = -h;
        }
        let oh = wo.dot(h);
        let ih = wi.dot(h);
        if oh <= 0.0 || ih >= 0.0 {
            return None;
        }
        let fr = dielectric_fresnel(oh, eta_ratio);
        if fr >= 1.0 {
            return None; // total internal reflection: nothing transmits
        }
        let nh = n.dot(h);
        let d = ggx_d(nh, a2);
        let denom = oh + rel_eta * ih;
        let denom2 = denom * denom;
        if denom2 < 1e-12 {
            return None;
        }
        // Walter eq. 21 (BTDF) and eq. 17 (the half-vector Jacobian that
        // turns the VNDF pdf into a solid-angle pdf over wi).
        let f = (1.0 - fr) * d * smith_g2(no, ni_abs, a2) * oh * ih.abs() * rel_eta * rel_eta
            / (no * ni_abs * denom2);
        let jacobian = rel_eta * rel_eta * ih.abs() / denom2;
        let pdf = (1.0 - fr) * d * smith_g1(no, a2) * (oh / no) * jacobian;
        Some(DielectricLobe { f, pdf })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::spectrum::LAMBDA_D;

    fn flat_hit(front_face: bool) -> HitRecord {
        HitRecord {
            t: 1.0,
            p: Vec3::ZERO,
            normal: Vec3::new(0.0, 1.0, 0.0),
            tangent: Vec3::new(1.0, 0.0, 0.0),
            front_face,
            u: 0.0,
            v: 0.0,
            area: 1.0,
            material: 0,
            prim: u32::MAX,
        }
    }

    #[test]
    fn lambertian_sample_eval_pdf_are_consistent() {
        let mut sampler = Sampler::random(1);
        let albedo = Color::new(0.5, 0.4, 0.3);
        let m = Material::Lambertian {
            albedo: albedo.into(),
            normal: None,
        };
        let hit = flat_hit(true);
        let wo = Vec3::new(0.0, 1.0, 0.0);
        for _ in 0..1000 {
            let s = m.sample(wo, &hit, LAMBDA_D, &mut sampler).unwrap();
            assert!(s.wi.dot(hit.normal) >= -1e-4);
            assert!((s.f - m.eval(wo, s.wi, &hit, LAMBDA_D)).length() < 1e-5);
            assert!((s.pdf - m.pdf(wo, s.wi, &hit, LAMBDA_D)).abs() < 1e-5);
            assert!(!s.specular);
        }
    }

    #[test]
    fn plastic_sample_eval_pdf_are_consistent() {
        let mut sampler = Sampler::random(11);
        let m = Material::Plastic {
            albedo: Color::new(0.2, 0.5, 0.8).into(),
            roughness: 0.27,
            specular: 0.06,
            normal: None,
        };
        let hit = flat_hit(true);
        let wo = Vec3::new(0.4, 1.0, -0.2).normalize();
        assert!(!m.is_specular());
        for _ in 0..1000 {
            let Some(s) = m.sample(wo, &hit, LAMBDA_D, &mut sampler) else {
                continue; // reflected below the surface
            };
            assert!(s.wi.dot(hit.normal) >= -1e-4);
            assert!((s.f - m.eval(wo, s.wi, &hit, LAMBDA_D)).length() < 1e-4);
            assert!((s.pdf - m.pdf(wo, s.wi, &hit, LAMBDA_D)).abs() < 1e-4);
            assert!(!s.specular);
        }
    }

    #[test]
    fn plastic_is_energy_conserving() {
        // Even with a white base and a full-strength coat, the directional
        // albedo (single scattering) must not exceed ~1.
        let mut sampler = Sampler::random(13);
        let m = Material::Plastic {
            albedo: Color::ONE.into(),
            roughness: 0.27,
            specular: 1.0,
            normal: None,
        };
        let hit = flat_hit(true);
        let wo = Vec3::new(0.3, 1.0, 0.1).normalize();
        let n = 20_000;
        let mut sum = Color::ZERO;
        for _ in 0..n {
            if let Some(s) = m.sample(wo, &hit, LAMBDA_D, &mut sampler) {
                sum += s.f * s.wi.dot(hit.normal).max(0.0) / s.pdf;
            }
        }
        let albedo = sum / n as Float;
        assert!(
            albedo.max_component() <= 1.05,
            "directional albedo too high: {albedo:?}"
        );
    }

    #[test]
    fn emissive_does_not_scatter_but_emits() {
        let mut sampler = Sampler::random(2);
        let m = Material::Emissive {
            emit: Color::new(4.0, 4.0, 4.0),
        };
        assert!(m
            .sample(
                Vec3::new(0.0, 1.0, 0.0),
                &flat_hit(true),
                LAMBDA_D,
                &mut sampler
            )
            .is_none());
        assert_eq!(m.emitted(), Color::new(4.0, 4.0, 4.0));
        assert!(!m.is_specular());
    }

    #[test]
    fn mirror_metal_reflects_and_is_specular() {
        let mut sampler = Sampler::random(3);
        let m = Material::Metal {
            albedo: Color::ONE.into(),
            roughness: 0.0,
            normal: None,
        };
        assert!(m.is_specular());
        let wo = Vec3::new(-1.0, 1.0, 0.0).normalize();
        let s = m
            .sample(wo, &flat_hit(true), LAMBDA_D, &mut sampler)
            .unwrap();
        assert!(s.specular);
        let expected = Vec3::new(1.0, 1.0, 0.0).normalize();
        assert!((s.wi - expected).length() < 1e-4);
    }

    #[test]
    fn rough_metal_is_non_specular_and_energy_conserving() {
        // With F0 = 1 (white), the single-scattering throughput weight is
        // G2/G1 <= 1, so the directional albedo must not exceed ~1.
        let mut sampler = Sampler::random(7);
        let m = Material::Metal {
            albedo: Color::ONE.into(),
            roughness: 0.3,
            normal: None,
        };
        assert!(!m.is_specular());
        let hit = flat_hit(true);
        let wo = Vec3::new(0.3, 1.0, 0.0).normalize();
        let n = 40_000;
        let mut sum = 0.0;
        for _ in 0..n {
            // Below-surface microfacet reflections are legitimately rejected
            // (None) and contribute zero to the directional albedo.
            if let Some(s) = m.sample(wo, &hit, LAMBDA_D, &mut sampler) {
                assert!(!s.specular);
                let cos = s.wi.dot(hit.normal).max(0.0);
                let weight = (s.f * (cos / s.pdf)).max_component();
                assert!(weight.is_finite() && weight <= 1.02, "weight {weight} > 1");
                sum += weight;
            }
        }
        let directional_albedo = sum / n as Float;
        assert!(
            directional_albedo > 0.3 && directional_albedo <= 1.02,
            "directional albedo out of range: {directional_albedo}"
        );
    }

    #[test]
    fn dielectric_is_specular_and_white() {
        let mut sampler = Sampler::random(4);
        let m = Material::Dielectric {
            ior: 1.5,
            absorption: Color::ZERO,
            roughness: 0.0,
            dispersion: 0.0,
        };
        let s = m
            .sample(
                Vec3::new(0.0, 1.0, 0.0),
                &flat_hit(true),
                LAMBDA_D,
                &mut sampler,
            )
            .unwrap();
        assert!(s.specular);
        assert_eq!(s.f, Color::ONE);
    }

    #[test]
    fn rough_dielectric_sample_eval_pdf_are_consistent() {
        let mut sampler = Sampler::random(11);
        let m = Material::Dielectric {
            ior: 1.5,
            absorption: Color::ZERO,
            roughness: 0.3,
            dispersion: 0.0,
        };
        assert!(!m.is_specular());
        for &front in &[true, false] {
            let hit = flat_hit(front);
            let wo = Vec3::new(0.4, 1.0, 0.1).normalize();
            let (mut refl, mut trans) = (0u32, 0u32);
            for _ in 0..2000 {
                let Some(s) = m.sample(wo, &hit, LAMBDA_D, &mut sampler) else {
                    continue;
                };
                assert!(!s.specular);
                if s.wi.dot(hit.normal) > 0.0 {
                    refl += 1;
                } else {
                    trans += 1;
                }
                let f = m.eval(wo, s.wi, &hit, LAMBDA_D);
                assert!(
                    (s.f - f).length() <= 1e-3 * s.f.length().max(1.0),
                    "eval disagrees with sample: {:?} vs {f:?}",
                    s.f
                );
                let pdf = m.pdf(wo, s.wi, &hit, LAMBDA_D);
                assert!(
                    ((s.pdf - pdf) / s.pdf).abs() < 1e-3,
                    "pdf disagrees with sample: {} vs {pdf}",
                    s.pdf
                );
            }
            assert!(
                refl > 20,
                "expected some reflection (front={front}): {refl}"
            );
            assert!(
                trans > 500,
                "expected mostly transmission at near-normal incidence (front={front}): {trans}"
            );
        }
    }

    #[test]
    fn rough_dielectric_is_energy_conserving() {
        // With VNDF sampling the per-sample throughput weight is G2/G1 <= 1
        // for both the reflection and transmission lobes.
        let mut sampler = Sampler::random(13);
        for &roughness in &[0.1, 0.3, 0.6] {
            let m = Material::Dielectric {
                ior: 1.5,
                absorption: Color::ZERO,
                roughness,
                dispersion: 0.0,
            };
            let hit = flat_hit(true);
            let wo = Vec3::new(0.3, 1.0, 0.0).normalize();
            let n = 20_000;
            let mut sum = 0.0;
            for _ in 0..n {
                if let Some(s) = m.sample(wo, &hit, LAMBDA_D, &mut sampler) {
                    let cos = s.wi.dot(hit.normal).abs();
                    let weight = (s.f * (cos / s.pdf)).max_component();
                    assert!(
                        weight.is_finite() && weight <= 1.02,
                        "weight {weight} > 1 at roughness {roughness}"
                    );
                    sum += weight;
                }
            }
            let albedo = sum / n as Float;
            assert!(
                albedo > 0.5 && albedo <= 1.02,
                "directional albedo out of range at roughness {roughness}: {albedo}"
            );
        }
    }

    #[test]
    fn dielectric_transmittance_follows_beer_lambert() {
        let clear = Material::Dielectric {
            ior: 1.5,
            absorption: Color::ZERO,
            roughness: 0.0,
            dispersion: 0.0,
        };
        assert_eq!(clear.transmittance(3.0), Color::ONE);

        let tinted = Material::Dielectric {
            ior: 1.5,
            absorption: Color::new(0.5, 0.0, 2.0),
            roughness: 0.0,
            dispersion: 0.0,
        };
        let t = tinted.transmittance(2.0);
        assert!((t.x - (-1.0 as Float).exp()).abs() < 1e-6);
        assert!((t.y - 1.0).abs() < 1e-6);
        assert!((t.z - (-4.0 as Float).exp()).abs() < 1e-6);

        // Non-dielectrics never absorb.
        let diffuse = Material::Lambertian {
            albedo: Color::ONE.into(),
            normal: None,
        };
        assert_eq!(diffuse.transmittance(10.0), Color::ONE);
    }

    #[test]
    fn normal_map_flat_is_identity_and_tilts() {
        let hit = flat_hit(true); // normal (0,1,0), tangent (1,0,0)
                                  // A flat map (0.5,0.5,1.0) decodes to (0,0,1): no change.
        let flat = Texture::Constant(Color::new(0.5, 0.5, 1.0));
        assert!((apply_normal_map(&hit, &flat) - hit.normal).length() < 1e-5);

        // A map leaning toward +U tilts the shading normal toward the tangent.
        let tilt = Texture::Constant(Color::new(0.85, 0.5, 1.0));
        let n = apply_normal_map(&hit, &tilt);
        assert!(
            n.dot(hit.tangent) > 0.1,
            "normal should tilt toward +tangent"
        );
        assert!(n.dot(hit.normal) > 0.0, "normal should still face outward");
        assert!(
            (n.length() - 1.0).abs() < 1e-5,
            "perturbed normal must be unit"
        );
    }
}
