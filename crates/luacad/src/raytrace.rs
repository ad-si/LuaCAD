//! Path-traced rendering to PNG via the vendored `prime-core` crate.
//!
//! Shares the scene-collection pipeline with the rasterizer in `render.rs`
//! (per-subtree colors, optional smooth vertex normals, camera framing) but
//! replaces its fixed-function shading with physically based path tracing:
//! a large area light in the studio's key-light direction plus a uniform
//! environment standing in for the ambient/fill terms, which yields soft
//! shadows, ambient occlusion, and indirect bounce light.

use crate::geometry::CsgGeometry;
use crate::material::MaterialSpec;
use crate::render::{
  self, BG_COLOR, CAMERA_AZIMUTH, CAMERA_ELEVATION, SmoothTriangle,
};
use prime_core::prelude::*;
use std::collections::HashMap;
use std::io::BufWriter;
use std::path::Path;

const WIDTH: usize = 1024;
const HEIGHT: usize = 1024;
const SAMPLES_PER_PIXEL: usize = 128;
const MAX_DEPTH: usize = 8;

/// Output transfer gamma. Albedos and the background are linearized with the
/// same exponent, so the background round-trips to exactly `BG_COLOR`.
const GAMMA: f32 = 2.2;

/// Vertical field of view. The camera distance is derived from it so the
/// model fills the frame like the rasterizer's orthographic framing
/// (half-height = 0.75 × max extent at the target plane).
const VFOV_DEG: f32 = 30.0;

/// Key light placement: distance and half-size relative to the scene's max
/// extent. Half-size / distance ≈ 0.15 rad of apparent radius — large enough
/// for clearly soft shadows.
const KEY_DISTANCE_FACTOR: f32 = 10.0;
const KEY_SIZE_FACTOR: f32 = 0.15;

/// Effective diffuse strength of the key light, in the same units as the
/// studio's 0.9-diffuse key: a surface facing the light receives
/// `KEY_STRENGTH × albedo` of outgoing radiance from it. Lower than 0.9
/// because the uniform environment already contributes ~0.7 × albedo of
/// ambient, where the studio's ambient is 0.35.
const KEY_STRENGTH: f32 = 0.6;

// The implicit object material (a plastic coat approximating the studio's
// Blinn-Phong highlight) lives in `crate::material`: shininess 25 ≈ GGX
// roughness √(2/(25+2)) ≈ 0.27, F0 0.06.

/// Render geometries to a PNG file using path tracing.
///
/// `smooth` selects smooth vertex normals (averaged across faces meeting at
/// less than 45°), exactly as in [`render::render_to_png`].
pub fn render_to_png(
  geometries: &[CsgGeometry],
  output: &Path,
  smooth: bool,
) -> Result<(), String> {
  let blockers = crate::export::geometries_unsupported_for_display(geometries);
  if !blockers.is_empty() {
    return Err(crate::export::describe_unsupported(&blockers));
  }

  let triangles = render::collect_smooth_triangles(geometries, smooth);
  if triangles.is_empty() {
    return Err("No geometry to render".to_string());
  }

  let (bb_min, bb_max) = render::bounding_box(&triangles);
  let center = Vec3::new(
    (bb_min[1] + bb_max[1]) * 0.5,
    (bb_min[2] + bb_max[2]) * 0.5,
    (bb_min[0] + bb_max[0]) * 0.5,
  );
  let max_extent = (bb_max[0] - bb_min[0])
    .max(bb_max[1] - bb_min[1])
    .max(bb_max[2] - bb_min[2]);

  let (materials, mut primitives) = build_primitives(&triangles, smooth);
  let camera = build_camera(center, max_extent);

  // Key light in the studio's key direction (eye space (1, 1, 0.5)): rotate
  // it into world space with the camera basis, mirroring `lights_to_world`.
  let forward = (center - camera.look_from).normalize();
  let right = forward.cross(Vec3::new(0.0, 1.0, 0.0)).normalize();
  let up = right.cross(forward);
  let key_dir = (right * 1.0 + up * 1.0 - forward * 0.5).normalize();

  let mut materials = materials;
  add_key_light(&mut materials, &mut primitives, center, max_extent, key_dir);

  let background = Background::Solid(srgb_bytes_to_linear(BG_COLOR));
  let scene = Scene::new(materials, primitives, camera, background);

  let settings = RenderSettings {
    width: WIDTH,
    height: HEIGHT,
    samples_per_pixel: SAMPLES_PER_PIXEL,
    max_depth: MAX_DEPTH,
    seed: 0,
    low_discrepancy: true,
    // Paths that hit the small bright key light through an unlucky bounce
    // produce fireflies; clamping trades a little energy for a clean image.
    firefly_clamp: 20.0,
    // Clamp (not Reinhard/ACES) so the background resolves to exactly
    // `BG_COLOR`, matching the rasterizer and the studio.
    tonemap: Tonemap::Clamp,
    gamma: GAMMA,
  };

  let bytes = render_to_srgb(&scene, &settings, || {});
  write_png(&bytes, output)
}

/// Perspective camera on the studio's default orbit (azimuth −30°,
/// elevation 30°), far enough away to frame the model like the
/// rasterizer's orthographic view.
fn build_camera(center: Vec3, max_extent: f32) -> CameraConfig {
  let az = CAMERA_AZIMUTH.to_radians();
  let el = CAMERA_ELEVATION.to_radians();
  let half_height = max_extent * 0.75;
  // 1.05: slack for perspective making near geometry larger.
  let distance = half_height / (VFOV_DEG.to_radians() * 0.5).tan() * 1.05;

  let offset = Vec3::new(
    distance * el.cos() * az.sin(),
    distance * el.sin(),
    distance * el.cos() * az.cos(),
  );

  CameraConfig {
    look_from: center + offset,
    look_at: center,
    vup: Vec3::new(0.0, 1.0, 0.0),
    vfov: VFOV_DEG,
    aperture: 0.0,
    focus_dist: None,
  }
}

/// Absorption density of tinted glass, per world unit: chosen so the tint
/// saturates over roughly a 10-unit-thick part.
const GLASS_ABSORPTION: f32 = 0.1;

/// Map a LuaCAD material + resolved color onto a prime-core BSDF.
fn to_prime_material(spec: &MaterialSpec, color: [f32; 3]) -> Material {
  use crate::material::MaterialKind;
  let albedo = srgb_to_linear(color);
  match spec.kind {
    MaterialKind::Matte => Material::Lambertian {
      albedo: albedo.into(),
      normal: None,
    },
    MaterialKind::Plastic => Material::Plastic {
      albedo: albedo.into(),
      roughness: spec.roughness,
      specular: spec.specular,
      normal: None,
    },
    MaterialKind::Metal => Material::Metal {
      albedo: albedo.into(),
      roughness: spec.roughness,
      normal: None,
    },
    // The color tints the interior: Beer-Lambert absorbs the complement.
    MaterialKind::Glass => Material::Dielectric {
      ior: spec.ior,
      absorption: (Color::splat(1.0) - albedo) * GLASS_ABSORPTION,
      roughness: spec.roughness,
      dispersion: 0.0,
    },
    MaterialKind::Emissive => Material::Emissive {
      emit: albedo * spec.strength,
    },
  }
}

/// Convert the collected triangles into prime-core primitives, deduplicating
/// one BSDF per distinct (color, material) pair.
fn build_primitives(
  triangles: &[SmoothTriangle],
  smooth: bool,
) -> (Vec<Material>, Vec<Primitive>) {
  let mut materials: Vec<Material> = Vec::new();
  let mut by_key: HashMap<([u32; 3], [u32; 5]), MaterialId> = HashMap::new();
  let mut primitives = Vec::with_capacity(triangles.len());

  for tri in triangles {
    let key = (tri.color.map(f32::to_bits), tri.material.key());
    let material = *by_key.entry(key).or_insert_with(|| {
      materials.push(to_prime_material(&tri.material, tri.color));
      materials.len() - 1
    });

    let v = tri.verts.map(render::cad_to_gl).map(v3);
    let mut prim = Triangle::new(v[0], v[1], v[2], material);
    if smooth {
      prim.normals = Some(tri.normals.map(render::cad_to_gl).map(v3));
    }
    primitives.push(Primitive::from(prim));
  }

  (materials, primitives)
}

/// Add the emissive key-light quad: two triangles far outside the scene,
/// facing the model center.
fn add_key_light(
  materials: &mut Vec<Material>,
  primitives: &mut Vec<Primitive>,
  center: Vec3,
  max_extent: f32,
  key_dir: Vec3,
) {
  let distance = max_extent * KEY_DISTANCE_FACTOR;
  let half_size = distance * KEY_SIZE_FACTOR;

  // A diffuse surface facing the light sees radiance L over solid angle
  // Ω ≈ (2·half_size)² / distance², reflecting L·Ω/π of it. Choose L so that
  // equals KEY_STRENGTH.
  let omega = (2.0 * KEY_SIZE_FACTOR).powi(2);
  let radiance = KEY_STRENGTH * std::f32::consts::PI / omega;

  materials.push(Material::Emissive {
    emit: Color::splat(radiance),
  });
  let material = materials.len() - 1;

  let light_center = center + key_dir * distance;
  let n = -key_dir; // facing the model
  let u = n.cross(Vec3::new(0.0, 1.0, 0.0)).normalize() * half_size;
  let v = n.cross(u).normalize() * half_size;
  let corners = [
    light_center - u - v,
    light_center + u - v,
    light_center + u + v,
    light_center - u + v,
  ];
  primitives.push(Primitive::from(Triangle::new(
    corners[0], corners[1], corners[2], material,
  )));
  primitives.push(Primitive::from(Triangle::new(
    corners[0], corners[2], corners[3], material,
  )));
}

fn v3(a: [f32; 3]) -> Vec3 {
  Vec3::new(a[0], a[1], a[2])
}

/// sRGB → linear with the same exponent the output transform inverts.
fn srgb_to_linear(c: [f32; 3]) -> Color {
  Color::new(c[0].powf(GAMMA), c[1].powf(GAMMA), c[2].powf(GAMMA))
}

fn srgb_bytes_to_linear(c: [u8; 3]) -> Color {
  srgb_to_linear([
    c[0] as f32 / 255.0,
    c[1] as f32 / 255.0,
    c[2] as f32 / 255.0,
  ])
}

/// Write interleaved RGB8 bytes as a PNG.
fn write_png(bytes: &[u8], path: &Path) -> Result<(), String> {
  let file = std::fs::File::create(path)
    .map_err(|e| format!("Failed to create {}: {e}", path.display()))?;
  let writer = BufWriter::new(file);

  let mut encoder = png::Encoder::new(writer, WIDTH as u32, HEIGHT as u32);
  encoder.set_color(png::ColorType::Rgb);
  encoder.set_depth(png::BitDepth::Eight);

  let mut writer = encoder
    .write_header()
    .map_err(|e| format!("PNG header error: {e}"))?;
  writer
    .write_image_data(bytes)
    .map_err(|e| format!("PNG write error: {e}"))?;

  Ok(())
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn a_colored_cube_renders_object_pixels_on_the_studio_background() {
    let geometries =
      crate::lua_engine::execute_lua("render(cube({size = {10, 10, 10}}))")
        .expect("Lua execution failed");
    let dir = std::env::temp_dir();
    let path = dir.join("luacad_raytrace_smoke_test.png");

    render_to_png(&geometries, &path, false).expect("render failed");

    let file = std::fs::File::open(&path).expect("PNG missing");
    let decoder = png::Decoder::new(std::io::BufReader::new(file));
    let mut reader = decoder.read_info().expect("PNG unreadable");
    let mut buf = vec![0; reader.output_buffer_size().unwrap()];
    let info = reader.next_frame(&mut buf).expect("PNG frame unreadable");
    assert_eq!((info.width, info.height), (WIDTH as u32, HEIGHT as u32));

    // Corner pixel is pure background; the center shows the cube.
    assert_eq!(&buf[0..3], &BG_COLOR);
    let center = ((HEIGHT / 2) * WIDTH + WIDTH / 2) * 3;
    assert_ne!(&buf[center..center + 3], &BG_COLOR);

    std::fs::remove_file(&path).ok();
  }
}
