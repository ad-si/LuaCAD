//! Path-traced rendering to PNG via the vendored `prime-core` crate.
//!
//! Shares the scene-collection pipeline with the rasterizer in `render.rs`
//! (per-subtree colors, smooth vertex normals, camera framing) but
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

const WIDTH: usize = 2048;
const HEIGHT: usize = 1536;
const DEFAULT_SAMPLES_PER_PIXEL: usize = 128;
const MAX_DEPTH: usize = 8;

/// Output transfer gamma. Albedos and the background are linearized with the
/// same exponent, so the background round-trips to exactly `BG_COLOR`.
const GAMMA: f32 = 2.2;

/// Vertical field of view. The camera distance is the smallest one at
/// which every vertex fits inside both FOV axes (see [`build_camera`]),
/// matching the rasterizer's fit-to-extent framing.
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
/// Always shades with smooth vertex normals (averaged across faces meeting
/// at less than 45°, so creases stay sharp): faceted round surfaces are a
/// tessellation-debugging view, which the rasterizer covers.
///
/// `samples` overrides the samples-per-pixel count (default: 128); noise
/// falls with the square root of the sample count. `camera` overrides the
/// (azimuth, elevation) orbit angles in degrees (default: the studio's
/// initial view).
pub fn render_to_png(
  geometries: &[CsgGeometry],
  output: &Path,
  samples: Option<usize>,
  camera: Option<(f32, f32)>,
) -> Result<(), String> {
  let bytes =
    render_to_rgb8(geometries, WIDTH, HEIGHT, samples, camera, None, || {})?;
  write_png(&bytes, WIDTH, HEIGHT, output)
}

/// Explicit viewport framing for [`render_to_rgb8`], replacing the default
/// fit-to-extent framing so the render matches an interactive view's zoom
/// and pan.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Framing {
  /// Orbit target in CAD coordinates
  pub target: [f32; 3],
  /// Camera distance from the target, in world units
  pub distance: f32,
  /// Vertical field of view in degrees
  pub vfov: f32,
}

/// Render geometries to interleaved RGB8 bytes using path tracing, at an
/// arbitrary resolution. With `framing: None`, the camera distance fits the
/// model extent on the given (azimuth, elevation) orbit; a [`Framing`]
/// reproduces a specific zoom and pan instead. See [`render_to_png`] for
/// the shared semantics of `samples` and `camera`.
///
/// `on_row_done` is invoked once per completed scanline (from worker
/// threads), so a caller can report progress out of `height` rows; pass
/// `|| {}` to ignore it.
pub fn render_to_rgb8<F: Fn() + Sync + Send>(
  geometries: &[CsgGeometry],
  width: usize,
  height: usize,
  samples: Option<usize>,
  camera: Option<(f32, f32)>,
  framing: Option<Framing>,
  on_row_done: F,
) -> Result<Vec<u8>, String> {
  let blockers = crate::export::geometries_unsupported_for_display(geometries);
  if !blockers.is_empty() {
    return Err(crate::export::describe_unsupported(&blockers));
  }

  let triangles = render::collect_smooth_triangles(geometries, true);
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

  let (materials, mut primitives) = build_primitives(&triangles);
  let (azimuth, elevation) =
    camera.unwrap_or((CAMERA_AZIMUTH, CAMERA_ELEVATION));
  let camera = match framing {
    Some(f) => framed_camera(&f, azimuth, elevation),
    None => build_camera(center, &triangles, azimuth, elevation, width, height),
  };

  // Key light in the studio's key direction (eye space (1, 1, 0.5)): rotate
  // it into world space with the camera basis, mirroring `lights_to_world`.
  let forward = (camera.look_at - camera.look_from).normalize();
  let right = forward.cross(Vec3::new(0.0, 1.0, 0.0)).normalize();
  let up = right.cross(forward);
  let key_dir = (right * 1.0 + up * 1.0 - forward * 0.5).normalize();

  let mut materials = materials;
  add_key_light(&mut materials, &mut primitives, center, max_extent, key_dir);

  let background = Background::Solid(srgb_bytes_to_linear(BG_COLOR));
  let scene = Scene::new(materials, primitives, camera, background);

  let settings = RenderSettings {
    width,
    height,
    samples_per_pixel: samples.unwrap_or(DEFAULT_SAMPLES_PER_PIXEL),
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

  Ok(render_to_srgb(&scene, &settings, on_row_done))
}

/// Perspective camera on the given orbit angles (in degrees) at an explicit
/// [`Framing`], reproducing an interactive viewport's zoom and pan.
fn framed_camera(
  framing: &Framing,
  azimuth: f32,
  elevation: f32,
) -> CameraConfig {
  let az = azimuth.to_radians();
  let el = elevation.to_radians();

  // Unit direction from the look-at target toward the camera
  let dir = Vec3::new(el.cos() * az.sin(), el.sin(), el.cos() * az.cos());
  let target = v3(render::cad_to_gl(framing.target));

  CameraConfig {
    look_from: target + dir * framing.distance,
    look_at: target,
    vup: Vec3::new(0.0, 1.0, 0.0),
    vfov: framing.vfov,
    aperture: 0.0,
    focus_dist: None,
  }
}

/// Perspective camera on the given orbit angles (in degrees), at the
/// smallest distance where every vertex fits inside both the vertical and
/// horizontal FOV (padded by [`render::FRAME_MARGIN`]), matching the
/// rasterizer's fit-to-extent framing.
fn build_camera(
  center: Vec3,
  triangles: &[SmoothTriangle],
  azimuth: f32,
  elevation: f32,
  width: usize,
  height: usize,
) -> CameraConfig {
  let az = azimuth.to_radians();
  let el = elevation.to_radians();

  // Unit direction from the look-at target toward the camera, and the
  // camera's right/up basis (forward is -dir).
  let dir = Vec3::new(el.cos() * az.sin(), el.sin(), el.cos() * az.cos());
  let vup = Vec3::new(0.0, 1.0, 0.0);
  let right = (-dir).cross(vup).normalize();
  let up = right.cross(-dir);

  // A vertex at offset p from the target needs the camera at least
  // `p·dir + |p·axis| / tan(fov/2)` away along `dir` to land inside that
  // FOV axis; take the max over all vertices and both axes.
  let tan_v = (VFOV_DEG.to_radians() * 0.5).tan();
  let tan_h = tan_v * (width as f32 / height as f32);
  let mut distance = 0.0_f32;
  for tri in triangles {
    for &v in &tri.verts {
      // CAD→GL: gl_x=cad_y, gl_y=cad_z, gl_z=cad_x
      let p = Vec3::new(v[1], v[2], v[0]) - center;
      let toward_cam = p.dot(dir);
      distance = distance
        .max(toward_cam + p.dot(up).abs() / tan_v)
        .max(toward_cam + p.dot(right).abs() / tan_h);
    }
  }
  distance *= render::FRAME_MARGIN;

  CameraConfig {
    look_from: center + dir * distance,
    look_at: center,
    vup,
    vfov: VFOV_DEG,
    aperture: 0.0,
    focus_dist: None,
  }
}

/// Absorption density of tinted glass, per world unit: chosen so the tint
/// saturates over roughly a 10-unit-thick part.
const GLASS_ABSORPTION: f32 = 0.1;

/// The albedo of a material: uniform, or procedural wood grain blending the
/// base color toward a darkened latewood variant of itself.
///
/// The grain axis is given in CAD coordinates; the texture is evaluated at
/// path-tracer hit points, which live in GL space, so the axis is swizzled
/// the same way as the geometry. The rasterizer's grain path
/// (`render::grain_color`) evaluates the identical scalar field, keeping the
/// two renderers' grain aligned.
fn albedo_texture(
  spec: &MaterialSpec,
  color: [f32; 3],
) -> prime_core::texture::Texture {
  use prime_core::texture::Texture;
  match spec.grain {
    None => Texture::Constant(srgb_to_linear(color)),
    Some(g) => Texture::Wood {
      early: srgb_to_linear(color),
      late: srgb_to_linear(color.map(|c| c * (1.0 - g.contrast))),
      frequency: 1.0 / g.ring_width.max(1e-3),
      distortion: g.distortion,
      axis: v3(render::cad_to_gl(g.axis)),
      offset: v3(render::cad_to_gl(g.offset)),
    },
  }
}

/// Map a LuaCAD material + resolved color onto a prime-core BSDF.
fn to_prime_material(spec: &MaterialSpec, color: [f32; 3]) -> Material {
  use crate::material::MaterialKind;
  let albedo = srgb_to_linear(color);
  match spec.kind {
    MaterialKind::Matte => Material::Lambertian {
      albedo: albedo_texture(spec, color),
      normal: None,
    },
    MaterialKind::Plastic => Material::Plastic {
      albedo: albedo_texture(spec, color),
      roughness: spec.roughness,
      specular: spec.specular,
      normal: None,
    },
    MaterialKind::Metal => Material::Metal {
      albedo: albedo_texture(spec, color),
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
) -> (Vec<Material>, Vec<Primitive>) {
  let mut materials: Vec<Material> = Vec::new();
  let mut by_key: HashMap<([u32; 3], [u32; 15]), MaterialId> = HashMap::new();
  let mut primitives = Vec::with_capacity(triangles.len());

  for tri in triangles {
    let key = (tri.color.map(f32::to_bits), tri.material.key());
    let material = *by_key.entry(key).or_insert_with(|| {
      materials.push(to_prime_material(&tri.material, tri.color));
      materials.len() - 1
    });

    let v = tri.verts.map(render::cad_to_gl).map(v3);
    let mut prim = Triangle::new(v[0], v[1], v[2], material);
    prim.normals = Some(tri.normals.map(render::cad_to_gl).map(v3));
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
fn write_png(
  bytes: &[u8],
  width: usize,
  height: usize,
  path: &Path,
) -> Result<(), String> {
  let file = std::fs::File::create(path)
    .map_err(|e| format!("Failed to create {}: {e}", path.display()))?;
  let writer = BufWriter::new(file);

  let mut encoder = png::Encoder::new(writer, width as u32, height as u32);
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
  fn rgb8_rendering_honors_custom_resolutions_and_reports_rows() {
    let geometries =
      crate::lua_engine::execute_lua("render(cube({size = {10, 10, 10}}))")
        .expect("Lua execution failed");

    let (width, height) = (64, 48);
    let rows = std::sync::atomic::AtomicUsize::new(0);
    let bytes =
      render_to_rgb8(&geometries, width, height, Some(2), None, None, || {
        rows.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
      })
      .expect("render failed");

    assert_eq!(bytes.len(), width * height * 3);
    assert_eq!(rows.load(std::sync::atomic::Ordering::Relaxed), height);

    // Corner pixel is pure background; the center shows the cube.
    assert_eq!(&bytes[0..3], &BG_COLOR);
    let center = ((height / 2) * width + width / 2) * 3;
    assert_ne!(&bytes[center..center + 3], &BG_COLOR);
  }

  #[test]
  fn explicit_framing_controls_zoom_and_pan() {
    // A 10-unit cube spanning 0..10 on each CAD axis
    let geometries =
      crate::lua_engine::execute_lua("render(cube({size = {10, 10, 10}}))")
        .expect("Lua execution failed");

    let object_pixels = |framing: Framing| {
      let bytes = render_to_rgb8(
        &geometries,
        64,
        48,
        Some(2),
        None,
        Some(framing),
        || {},
      )
      .expect("render failed");
      bytes.chunks(3).filter(|&px| px != BG_COLOR).count()
    };

    // Zoom: halving the distance grows the cube's screen coverage
    let near = object_pixels(Framing {
      target: [5.0; 3],
      distance: 25.0,
      vfov: 45.0,
    });
    let far = object_pixels(Framing {
      target: [5.0; 3],
      distance: 50.0,
      vfov: 45.0,
    });
    assert!(near > 2 * far, "near {near} px, far {far} px");

    // Pan: a target far off the model leaves only background
    let away = object_pixels(Framing {
      target: [500.0, 0.0, 0.0],
      distance: 25.0,
      vfov: 45.0,
    });
    assert_eq!(away, 0, "panned-away view still shows {away} object px");
  }

  #[test]
  fn a_colored_cube_renders_object_pixels_on_the_studio_background() {
    let geometries =
      crate::lua_engine::execute_lua("render(cube({size = {10, 10, 10}}))")
        .expect("Lua execution failed");
    let dir = std::env::temp_dir();
    let path = dir.join("luacad_raytrace_smoke_test.png");

    render_to_png(&geometries, &path, None, None).expect("render failed");

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
