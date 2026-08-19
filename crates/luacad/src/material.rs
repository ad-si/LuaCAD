//! Surface materials.
//!
//! A material describes how a surface scatters light. The path tracer maps
//! each kind onto one of prime-core's BSDFs; the rasterizer and the studio
//! preview approximate them with adjusted Blinn-Phong terms. Materials are
//! orthogonal to colors: `color()` picks the albedo, `material()` picks the
//! scattering behavior. Presets may carry a default color that is used only
//! when no explicit color is set.

use mlua::Value as LuaValue;

use crate::geometry::lua_val_to_f32;

/// How a surface scatters light.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum MaterialKind {
  /// Ideal diffuse reflector, no highlight.
  Matte,
  /// Diffuse base under a glossy specular coat (the default look).
  Plastic,
  /// Conductor: tinted mirror-like reflection, no diffuse term.
  Metal,
  /// Dielectric with refraction (path tracer) or a polished highlight
  /// (rasterizer/preview, which cannot refract).
  Glass,
  /// Light source: emits, does not scatter.
  Emissive,
}

/// A resolved material: kind plus its parameters.
///
/// All parameters are stored for every kind; each kind reads only the ones
/// that apply to it. This keeps the type `Copy` and trivially comparable.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MaterialSpec {
  pub kind: MaterialKind,
  /// GGX roughness in `[0, 1]` (plastic coat, metal, frosted glass).
  pub roughness: f32,
  /// Normal-incidence reflectance F0 of the plastic coat.
  pub specular: f32,
  /// Index of refraction (glass).
  pub ior: f32,
  /// Emission strength multiplier (emissive).
  pub strength: f32,
  /// Preset color, applied only when the object has no explicit `color()`.
  pub default_color: Option<[f32; 3]>,
}

/// Roughness/specular of the implicit material every object had before
/// `material()` existed (see `raytrace.rs`: shininess 25 ≈ roughness 0.27).
pub const DEFAULT_ROUGHNESS: f32 = 0.27;
pub const DEFAULT_SPECULAR: f32 = 0.06;

impl Default for MaterialSpec {
  fn default() -> Self {
    MaterialSpec {
      kind: MaterialKind::Plastic,
      roughness: DEFAULT_ROUGHNESS,
      specular: DEFAULT_SPECULAR,
      ior: 1.5,
      strength: 1.0,
      default_color: None,
    }
  }
}

impl MaterialSpec {
  /// Resolve a material preset by name.
  ///
  /// The base kinds (`matte`, `plastic`, `metal`, `glass`, `emissive`) carry
  /// no color; the convenience presets (`steel`, `chrome`, `gold`, `copper`,
  /// `brass`, `rubber`, `wood`, `ivory`) also set a default color used when
  /// the object has no explicit `color()`.
  pub fn named(name: &str) -> Option<MaterialSpec> {
    let base = MaterialSpec::default();
    let kind = |kind| MaterialSpec { kind, ..base };
    let c = |r: u8, g: u8, b: u8| {
      Some([r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0])
    };
    match name.to_lowercase().as_str() {
      "matte" => Some(kind(MaterialKind::Matte)),
      "plastic" => Some(kind(MaterialKind::Plastic)),
      "metal" => Some(MaterialSpec {
        kind: MaterialKind::Metal,
        roughness: 0.25,
        ..base
      }),
      "glass" => Some(MaterialSpec {
        kind: MaterialKind::Glass,
        roughness: 0.0,
        ..base
      }),
      "emissive" | "glow" => Some(kind(MaterialKind::Emissive)),
      "steel" => Some(MaterialSpec {
        kind: MaterialKind::Metal,
        roughness: 0.35,
        default_color: c(158, 160, 166),
        ..base
      }),
      "chrome" => Some(MaterialSpec {
        kind: MaterialKind::Metal,
        roughness: 0.05,
        default_color: c(217, 217, 217),
        ..base
      }),
      "gold" => Some(MaterialSpec {
        kind: MaterialKind::Metal,
        roughness: 0.25,
        default_color: c(255, 214, 107),
        ..base
      }),
      "copper" => Some(MaterialSpec {
        kind: MaterialKind::Metal,
        roughness: 0.3,
        default_color: c(242, 163, 137),
        ..base
      }),
      "brass" => Some(MaterialSpec {
        kind: MaterialKind::Metal,
        roughness: 0.3,
        default_color: c(232, 199, 107),
        ..base
      }),
      "rubber" => Some(MaterialSpec {
        kind: MaterialKind::Plastic,
        roughness: 0.7,
        specular: 0.03,
        default_color: c(38, 38, 38),
        ..base
      }),
      "wood" => Some(MaterialSpec {
        kind: MaterialKind::Plastic,
        roughness: 0.5,
        specular: 0.04,
        default_color: c(166, 124, 82),
        ..base
      }),
      "ivory" => Some(MaterialSpec {
        kind: MaterialKind::Plastic,
        roughness: 0.2,
        specular: 0.08,
        default_color: c(242, 236, 218),
        ..base
      }),
      _ => None,
    }
  }

  /// The kind as a lowercase name (for `tostring` and error messages).
  pub fn kind_name(&self) -> &'static str {
    match self.kind {
      MaterialKind::Matte => "matte",
      MaterialKind::Plastic => "plastic",
      MaterialKind::Metal => "metal",
      MaterialKind::Glass => "glass",
      MaterialKind::Emissive => "emissive",
    }
  }

  /// Bit-exact hash key (for material deduplication maps).
  pub fn key(&self) -> [u32; 5] {
    [
      self.kind as u32,
      self.roughness.to_bits(),
      self.specular.to_bits(),
      self.ior.to_bits(),
      self.strength.to_bits(),
    ]
  }

  /// Apply parameter overrides from a Lua options table
  /// (`roughness`, `specular`, `ior`, `strength`).
  fn apply_options(&mut self, t: &mlua::Table) {
    let get =
      |key: &str| t.get::<LuaValue>(key).ok().and_then(|v| lua_val_to_f32(&v));
    if let Some(v) = get("roughness") {
      self.roughness = v.clamp(0.0, 1.0);
    }
    if let Some(v) = get("specular") {
      self.specular = v.clamp(0.0, 1.0);
    }
    if let Some(v) = get("ior") {
      self.ior = v.max(1.0);
    }
    if let Some(v) = get("strength") {
      self.strength = v.max(0.0);
    }
  }
}

/// The legacy fixed-function highlight strength (`GL_SPECULAR` 0.4 in the
/// studio) that [`MaterialSpec::blinn_phong`] scales per material.
pub const BASE_SPECULAR_STRENGTH: f32 = 0.4;

/// Blinn-Phong approximation of a material for the rasterizer and the
/// studio's fixed-function preview: how strongly the diffuse and specular
/// terms contribute, and whether the highlight is tinted by the albedo
/// (metals) or white (dielectric coats).
pub struct BlinnPhong {
  pub diffuse_scale: f32,
  pub specular_strength: f32,
  pub shininess: f32,
  pub tinted_specular: bool,
}

/// Invert the roughness mapping the path tracer documents
/// (`roughness = √(2/(shininess+2))`), floored so polished materials
/// (roughness → 0) stay a finite highlight instead of a singular one.
pub fn shininess_from_roughness(roughness: f32) -> f32 {
  let r = roughness.max(0.04);
  2.0 / (r * r) - 2.0
}

impl MaterialSpec {
  /// The Blinn-Phong approximation of this material. Emissive surfaces are
  /// unlit and must be special-cased before shading; their parameters here
  /// are unused.
  pub fn blinn_phong(&self) -> BlinnPhong {
    let shininess = shininess_from_roughness(self.roughness);
    match self.kind {
      // No highlight at all.
      MaterialKind::Matte => BlinnPhong {
        diffuse_scale: 1.0,
        specular_strength: 0.0,
        shininess,
        tinted_specular: false,
      },
      // The legacy look: scale the legacy highlight strength by how far the
      // coat's F0 deviates from the implicit default.
      MaterialKind::Plastic => BlinnPhong {
        diffuse_scale: 1.0,
        specular_strength: BASE_SPECULAR_STRENGTH
          * (self.specular / DEFAULT_SPECULAR),
        shininess,
        tinted_specular: false,
      },
      // Conductor: reflection dominates and carries the albedo tint. The
      // diffuse term stands in for the environment reflections a simple
      // shader cannot produce, so it stays fairly strong.
      MaterialKind::Metal => BlinnPhong {
        diffuse_scale: 0.5,
        specular_strength: 0.9,
        shininess,
        tinted_specular: true,
      },
      // No refraction outside the path tracer — approximate as a polished
      // highlight over a dimmed body.
      MaterialKind::Glass => BlinnPhong {
        diffuse_scale: 0.55,
        specular_strength: 0.9,
        shininess,
        tinted_specular: false,
      },
      MaterialKind::Emissive => BlinnPhong {
        diffuse_scale: 0.0,
        specular_strength: 0.0,
        shininess,
        tinted_specular: false,
      },
    }
  }
}

/// Parse the arguments of the Lua `material()` method:
/// `material("metal")`, `material("metal", {roughness = 0.4})`, or
/// `material({kind = "metal", roughness = 0.4})`.
///
/// Unlike `color()`, an unknown material name is an error rather than a
/// silent fallback — a misspelled preset would otherwise go unnoticed.
pub fn parse_material_args(
  args: &mlua::MultiValue,
) -> mlua::Result<MaterialSpec> {
  let named = |name: &str| {
    MaterialSpec::named(name).ok_or_else(|| {
      mlua::Error::RuntimeError(format!(
        "unknown material \"{name}\" (expected matte, plastic, metal, glass, \
         emissive, steel, chrome, gold, copper, brass, rubber, wood, or \
         ivory)"
      ))
    })
  };
  match args.front() {
    Some(LuaValue::String(s)) => {
      let mut spec = named(&s.to_str()?)?;
      if let Some(LuaValue::Table(t)) = args.get(1) {
        spec.apply_options(t);
      }
      Ok(spec)
    }
    Some(LuaValue::Table(t)) => {
      let mut spec = match t.get::<Option<String>>("kind")? {
        Some(name) => named(&name)?,
        None => MaterialSpec::default(),
      };
      spec.apply_options(t);
      Ok(spec)
    }
    _ => Err(mlua::Error::RuntimeError(
      "material() expects a name, a name plus options table, or an options \
       table with a `kind` field"
        .to_string(),
    )),
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn named_presets_resolve() {
    assert_eq!(
      MaterialSpec::named("metal").unwrap().kind,
      MaterialKind::Metal
    );
    assert_eq!(
      MaterialSpec::named("Gold").unwrap().kind,
      MaterialKind::Metal
    );
    assert!(MaterialSpec::named("gold").unwrap().default_color.is_some());
    assert!(MaterialSpec::named("adamantium").is_none());
  }

  #[test]
  fn default_matches_legacy_look() {
    let spec = MaterialSpec::default();
    assert_eq!(spec.kind, MaterialKind::Plastic);
    assert_eq!(spec.roughness, DEFAULT_ROUGHNESS);
    assert_eq!(spec.specular, DEFAULT_SPECULAR);
  }
}
