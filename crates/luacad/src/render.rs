//! Software rasterizer for rendering geometry to PNG images.
//!
//! Uses the Manifold tessellation pipeline for correct CSG results,
//! with smooth vertex normals and Blinn-Phong shading matching the
//! studio's lighting setup.

use crate::export::{extract_manifold_mesh, materialize_scad_manifold};
use crate::geometry::CsgGeometry;
use crate::scad_export::ScadNode;
use std::collections::HashMap;
use std::io::BufWriter;
use std::path::Path;

/// Default output image dimensions.
const DEFAULT_WIDTH: u32 = 1024;
const DEFAULT_HEIGHT: u32 = 1024;

/// Supersampling factor for anti-aliasing (2x = 4 samples per pixel).
const SSAA: u32 = 2;

/// Default camera angles matching the studio's initial view.
const CAMERA_AZIMUTH: f32 = -30.0;
const CAMERA_ELEVATION: f32 = 30.0;

/// Background color (light gray, matching light theme).
const BG_COLOR: [u8; 3] = [242, 242, 242];

/// Default object color matching the studio (#3177be).
const DEFAULT_COLOR: [f32; 3] = [0.192, 0.467, 0.745];

/// Lighting parameters matching the studio's scene.rs setup.
const AMBIENT: f32 = 0.35;
const SPECULAR_STRENGTH: f32 = 0.4;
const SHININESS: f32 = 25.0;

struct Light {
  direction: [f32; 3], // normalized, towards the light
  diffuse: f32,
  specular: f32,
}

/// Studio lighting: key, fill, bottom.
fn studio_lights() -> [Light; 3] {
  [
    Light {
      direction: normalize([1.0, 1.0, 0.5]),
      diffuse: 0.9,
      specular: 0.6,
    },
    Light {
      direction: normalize([-1.0, 0.3, 0.5]),
      diffuse: 0.55,
      specular: 0.0,
    },
    Light {
      direction: normalize([0.0, -1.0, 0.0]),
      diffuse: 0.4,
      specular: 0.0,
    },
  ]
}

/// A triangle with per-vertex normals for smooth shading.
struct SmoothTriangle {
  /// Vertex positions in CAD space.
  verts: [[f32; 3]; 3],
  /// Per-vertex normals (averaged from adjacent faces).
  normals: [[f32; 3]; 3],
  /// Object color.
  color: [f32; 3],
}

struct Framebuffer {
  width: u32,
  height: u32,
  color: Vec<[u8; 3]>,
  depth: Vec<f32>,
}

impl Framebuffer {
  fn new(width: u32, height: u32) -> Self {
    let size = (width * height) as usize;
    Self {
      width,
      height,
      color: vec![BG_COLOR; size],
      depth: vec![f32::INFINITY; size],
    }
  }

  fn set_pixel(&mut self, x: u32, y: u32, depth: f32, color: [u8; 3]) {
    let idx = (y * self.width + x) as usize;
    if depth < self.depth[idx] {
      self.depth[idx] = depth;
      self.color[idx] = color;
    }
  }
}

/// Downsample a supersampled framebuffer to the target resolution using box filter.
fn downsample(src: &Framebuffer, dst_w: u32, dst_h: u32) -> Framebuffer {
  let scale_x = src.width / dst_w;
  let scale_y = src.height / dst_h;
  let samples = (scale_x * scale_y) as f32;
  let mut dst = Framebuffer::new(dst_w, dst_h);

  for dy in 0..dst_h {
    for dx in 0..dst_w {
      let mut r = 0.0_f32;
      let mut g = 0.0_f32;
      let mut b = 0.0_f32;
      for sy in 0..scale_y {
        for sx in 0..scale_x {
          let src_idx =
            ((dy * scale_y + sy) * src.width + dx * scale_x + sx) as usize;
          let pixel = src.color[src_idx];
          r += pixel[0] as f32;
          g += pixel[1] as f32;
          b += pixel[2] as f32;
        }
      }
      let dst_idx = (dy * dst_w + dx) as usize;
      dst.color[dst_idx] = [
        (r / samples) as u8,
        (g / samples) as u8,
        (b / samples) as u8,
      ];
    }
  }

  dst
}

/// Render geometries to a PNG file.
///
/// When `smooth` is false (default), uses flat per-face normals so the
/// tessellation is visible — useful for judging mesh quality / printability.
/// When `smooth` is true, averages vertex normals across adjacent coplanar
/// faces for a polished look.
pub fn render_to_png(
  geometries: &[CsgGeometry],
  output: &Path,
  smooth: bool,
) -> Result<(), String> {
  let triangles = collect_smooth_triangles(geometries, smooth);
  if triangles.is_empty() {
    return Err("No geometry to render".to_string());
  }

  // Compute bounding box for camera framing
  let (bb_min, bb_max) = bounding_box(&triangles);
  let center = [
    (bb_min[0] + bb_max[0]) * 0.5,
    (bb_min[1] + bb_max[1]) * 0.5,
    (bb_min[2] + bb_max[2]) * 0.5,
  ];
  let extent = [
    bb_max[0] - bb_min[0],
    bb_max[1] - bb_min[1],
    bb_max[2] - bb_min[2],
  ];
  let max_extent = extent[0].max(extent[1]).max(extent[2]);

  // Camera: orbit around center, same angles as studio default
  let az = CAMERA_AZIMUTH.to_radians();
  let el = CAMERA_ELEVATION.to_radians();
  let distance = max_extent * 1.8;

  let cam_x = distance * el.cos() * az.sin();
  let cam_y = distance * el.sin();
  let cam_z = distance * el.cos() * az.cos();

  // CAD→GL: gl_x=cad_y, gl_y=cad_z, gl_z=cad_x
  let gl_center = [center[1], center[2], center[0]];
  let cam_pos = [
    cam_x + gl_center[0],
    cam_y + gl_center[1],
    cam_z + gl_center[2],
  ];

  let view = look_at(cam_pos, gl_center, [0.0, 1.0, 0.0]);
  let half = max_extent * 0.75;
  let aspect = DEFAULT_WIDTH as f32 / DEFAULT_HEIGHT as f32;
  let proj = ortho(
    -half * aspect,
    half * aspect,
    -half,
    half,
    -distance * 10.0,
    distance * 10.0,
  );
  let mvp = mat4_mul(&proj, &view);

  // View direction for specular (camera looks towards -Z in view space,
  // but we need it in world space: normalize(cam_pos - gl_center))
  let view_dir = normalize(sub(cam_pos, gl_center));
  let lights = studio_lights();

  // Render at supersampled resolution for anti-aliased edges
  let ss_w = DEFAULT_WIDTH * SSAA;
  let ss_h = DEFAULT_HEIGHT * SSAA;
  let mut fb = Framebuffer::new(ss_w, ss_h);

  for tri in &triangles {
    let gl_verts = [
      cad_to_gl(tri.verts[0]),
      cad_to_gl(tri.verts[1]),
      cad_to_gl(tri.verts[2]),
    ];
    let gl_normals = [
      cad_to_gl(tri.normals[0]),
      cad_to_gl(tri.normals[1]),
      cad_to_gl(tri.normals[2]),
    ];

    let projected: [[f32; 4]; 3] = [
      transform_point(&mvp, gl_verts[0]),
      transform_point(&mvp, gl_verts[1]),
      transform_point(&mvp, gl_verts[2]),
    ];
    let screen: [[f32; 3]; 3] = projected.map(|p| {
      let w = p[3];
      [
        (p[0] / w * 0.5 + 0.5) * ss_w as f32,
        ((1.0 - p[1] / w) * 0.5) * ss_h as f32,
        p[2] / w,
      ]
    });

    rasterize_smooth_triangle(
      &mut fb,
      &screen,
      &gl_normals,
      tri.color,
      &view_dir,
      &lights,
    );
  }

  // Downsample to output resolution
  let out_fb = downsample(&fb, DEFAULT_WIDTH, DEFAULT_HEIGHT);
  write_png(&out_fb, output)
}

/// Compute Blinn-Phong shading for a given normal at a pixel.
fn shade_pixel(
  normal: [f32; 3],
  color: [f32; 3],
  view_dir: &[f32; 3],
  lights: &[Light; 3],
) -> [u8; 3] {
  let n = normalize(normal);

  let mut diffuse_total = AMBIENT;
  let mut specular_total = 0.0_f32;

  for light in lights {
    let ndotl = dot(n, light.direction);
    // Two-sided lighting
    let ndotl_abs = ndotl.abs();
    diffuse_total += light.diffuse * ndotl_abs;

    if light.specular > 0.0 {
      // Blinn-Phong: half-vector between light and view
      // Use the normal direction that faces the light for two-sided
      let oriented_n = if ndotl >= 0.0 {
        n
      } else {
        [-n[0], -n[1], -n[2]]
      };
      let h = normalize(add(light.direction, *view_dir));
      let ndoth = dot(oriented_n, h).max(0.0);
      specular_total += light.specular * ndoth.powf(SHININESS);
    }
  }

  let spec = specular_total * SPECULAR_STRENGTH;
  [
    ((color[0] * diffuse_total + spec).min(1.0) * 255.0) as u8,
    ((color[1] * diffuse_total + spec).min(1.0) * 255.0) as u8,
    ((color[2] * diffuse_total + spec).min(1.0) * 255.0) as u8,
  ]
}

/// Angle threshold for smooth normal averaging.
/// Edges steeper than this keep hard normals (preserves sharp creases).
const SMOOTH_ANGLE_COS: f32 = 0.707; // cos(45°)

/// Collect triangles with per-vertex normals, respecting per-subtree colors.
///
/// Walks the ScadNode tree and materializes colored subtrees independently
/// so each gets its correct color. Splits at Union and Color boundaries.
///
/// When `smooth` is true, normals are averaged between adjacent faces whose
/// angle difference is below the smooth threshold (preserving hard creases).
/// When `smooth` is false, each vertex gets its face normal (flat shading).
fn collect_smooth_triangles(
  geometries: &[CsgGeometry],
  smooth: bool,
) -> Vec<SmoothTriangle> {
  let mut all_triangles = Vec::new();

  for geom in geometries {
    let base_color = geom.color.unwrap_or(DEFAULT_COLOR);
    let scad = match geom.scad.as_ref() {
      Some(s) => s,
      None => continue,
    };

    let mut leaves: Vec<([f32; 3], ScadNode)> = Vec::new();
    collect_colored_leaves(scad, base_color, &[], &mut leaves);

    for (color, leaf) in &leaves {
      let manifold = materialize_scad_manifold(leaf);
      if manifold.num_tri() == 0 {
        continue;
      }
      materialize_mesh(&mut all_triangles, &manifold, *color, smooth);
    }
  }

  all_triangles
}

/// Recursively walk a ScadNode tree, splitting at Union and Color boundaries,
/// collecting `(color, leaf_node)` pairs where each leaf is a non-splittable
/// subtree wrapped in any ancestor transforms.
///
/// Unions are split: each child is visited independently.
/// Colors update the inherited color.
/// Transforms accumulate: leaf nodes are wrapped in all ancestor transforms.
/// Everything else (Difference, Intersection, primitives) is a leaf.
fn collect_colored_leaves(
  node: &ScadNode,
  color: [f32; 3],
  wrappers: &[WrapFn],
  out: &mut Vec<([f32; 3], ScadNode)>,
) {
  match node {
    ScadNode::Color { r, g, b, child, .. } => {
      collect_colored_leaves(child, [*r, *g, *b], wrappers, out);
    }

    ScadNode::Union(children) => {
      for child in children {
        collect_colored_leaves(child, color, wrappers, out);
      }
    }

    // Single-child transforms: push onto the wrapper stack and recurse
    ScadNode::Translate { x, y, z, child } => {
      let mut w = wrappers.to_vec();
      w.push(WrapFn::Translate(*x, *y, *z));
      collect_colored_leaves(child, color, &w, out);
    }
    ScadNode::Rotate { x, y, z, child } => {
      let mut w = wrappers.to_vec();
      w.push(WrapFn::Rotate(*x, *y, *z));
      collect_colored_leaves(child, color, &w, out);
    }
    ScadNode::Scale { x, y, z, child } => {
      let mut w = wrappers.to_vec();
      w.push(WrapFn::Scale(*x, *y, *z));
      collect_colored_leaves(child, color, &w, out);
    }
    ScadNode::Mirror { x, y, z, child } => {
      let mut w = wrappers.to_vec();
      w.push(WrapFn::Mirror(*x, *y, *z));
      collect_colored_leaves(child, color, &w, out);
    }
    ScadNode::Multmatrix { matrix, child } => {
      let mut w = wrappers.to_vec();
      w.push(WrapFn::Multmatrix(*matrix));
      collect_colored_leaves(child, color, &w, out);
    }
    ScadNode::Render { convexity, child } => {
      let mut w = wrappers.to_vec();
      w.push(WrapFn::Render(*convexity));
      collect_colored_leaves(child, color, &w, out);
    }
    ScadNode::Modifier { kind, child } => {
      let mut w = wrappers.to_vec();
      w.push(WrapFn::Modifier(kind.clone()));
      collect_colored_leaves(child, color, &w, out);
    }

    // Leaf: wrap in accumulated transforms and emit
    _ => {
      let wrapped = apply_wrappers(node.clone(), wrappers);
      out.push((color, wrapped));
    }
  }
}

/// A deferred transform/wrapper to apply around a leaf node.
#[derive(Clone)]
enum WrapFn {
  Translate(f32, f32, f32),
  Rotate(f32, f32, f32),
  Scale(f32, f32, f32),
  Mirror(f32, f32, f32),
  Multmatrix([f32; 16]),
  Render(u32),
  Modifier(crate::scad_export::ModifierKind),
}

/// Wrap a node in accumulated transforms (innermost first → outermost last).
fn apply_wrappers(mut node: ScadNode, wrappers: &[WrapFn]) -> ScadNode {
  for w in wrappers.iter().rev() {
    node = match w {
      WrapFn::Translate(x, y, z) => ScadNode::Translate {
        x: *x,
        y: *y,
        z: *z,
        child: Box::new(node),
      },
      WrapFn::Rotate(x, y, z) => ScadNode::Rotate {
        x: *x,
        y: *y,
        z: *z,
        child: Box::new(node),
      },
      WrapFn::Scale(x, y, z) => ScadNode::Scale {
        x: *x,
        y: *y,
        z: *z,
        child: Box::new(node),
      },
      WrapFn::Mirror(x, y, z) => ScadNode::Mirror {
        x: *x,
        y: *y,
        z: *z,
        child: Box::new(node),
      },
      WrapFn::Multmatrix(m) => ScadNode::Multmatrix {
        matrix: *m,
        child: Box::new(node),
      },
      WrapFn::Render(c) => ScadNode::Render {
        convexity: *c,
        child: Box::new(node),
      },
      WrapFn::Modifier(k) => ScadNode::Modifier {
        kind: k.clone(),
        child: Box::new(node),
      },
    };
  }
  node
}

/// Materialize a Manifold's mesh into SmoothTriangles.
fn materialize_mesh(
  out: &mut Vec<SmoothTriangle>,
  manifold: &crate::export::Manifold,
  color: [f32; 3],
  smooth: bool,
) {
  let mesh = extract_manifold_mesh(manifold);

  // Precompute face normals (normalized)
  let face_normals: Vec<[f32; 3]> = mesh
    .triangles
    .iter()
    .map(|tri| {
      let v0 = mesh.vertices[tri[0] as usize];
      let v1 = mesh.vertices[tri[1] as usize];
      let v2 = mesh.vertices[tri[2] as usize];
      normalize(cross(sub(v1, v0), sub(v2, v0)))
    })
    .collect();

  // Build vertex→face map only when smooth shading is requested
  let vert_faces: Option<HashMap<[i32; 3], Vec<(usize, [f32; 3])>>> = if smooth
  {
    let mut map: HashMap<[i32; 3], Vec<(usize, [f32; 3])>> = HashMap::new();
    for (fi, tri) in mesh.triangles.iter().enumerate() {
      for &vi in tri {
        let key = quantize(mesh.vertices[vi as usize]);
        map.entry(key).or_default().push((fi, face_normals[fi]));
      }
    }
    Some(map)
  } else {
    None
  };

  for (fi, tri) in mesh.triangles.iter().enumerate() {
    let verts = [
      mesh.vertices[tri[0] as usize],
      mesh.vertices[tri[1] as usize],
      mesh.vertices[tri[2] as usize],
    ];

    let normals = if let Some(ref vf) = vert_faces {
      let my_normal = face_normals[fi];
      let mut n = [[0.0f32; 3]; 3];
      for (vi, vert) in verts.iter().enumerate() {
        let key = quantize(*vert);
        let neighbors = &vf[&key];
        let mut accum = [0.0f32; 3];
        for &(_nfi, neighbor_normal) in neighbors {
          if dot(my_normal, neighbor_normal) >= SMOOTH_ANGLE_COS {
            accum[0] += neighbor_normal[0];
            accum[1] += neighbor_normal[1];
            accum[2] += neighbor_normal[2];
          }
        }
        n[vi] = normalize(accum);
      }
      n
    } else {
      let fn_ = face_normals[fi];
      [fn_, fn_, fn_]
    };

    out.push(SmoothTriangle {
      verts,
      normals,
      color,
    });
  }
}

/// Quantize a position to ~1μm grid for vertex welding.
fn quantize(pos: [f32; 3]) -> [i32; 3] {
  [
    (pos[0] * 1e4) as i32,
    (pos[1] * 1e4) as i32,
    (pos[2] * 1e4) as i32,
  ]
}

fn bounding_box(triangles: &[SmoothTriangle]) -> ([f32; 3], [f32; 3]) {
  let mut min = [f32::MAX; 3];
  let mut max = [f32::MIN; 3];
  for tri in triangles {
    for v in &tri.verts {
      for i in 0..3 {
        min[i] = min[i].min(v[i]);
        max[i] = max[i].max(v[i]);
      }
    }
  }
  (min, max)
}

// --- Vector math ---

fn cad_to_gl(v: [f32; 3]) -> [f32; 3] {
  [v[1], v[2], v[0]]
}

fn sub(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
  [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn add(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
  [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
  [
    a[1] * b[2] - a[2] * b[1],
    a[2] * b[0] - a[0] * b[2],
    a[0] * b[1] - a[1] * b[0],
  ]
}

fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
  a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn normalize(v: [f32; 3]) -> [f32; 3] {
  let len = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
  if len < 1e-12 {
    return [0.0, 0.0, 1.0];
  }
  [v[0] / len, v[1] / len, v[2] / len]
}

fn lerp3(
  a: [f32; 3],
  b: [f32; 3],
  c: [f32; 3],
  w0: f32,
  w1: f32,
  w2: f32,
) -> [f32; 3] {
  [
    a[0] * w0 + b[0] * w1 + c[0] * w2,
    a[1] * w0 + b[1] * w1 + c[1] * w2,
    a[2] * w0 + b[2] * w1 + c[2] * w2,
  ]
}

// --- Matrix math ---

type Mat4 = [f32; 16];

fn mat4_mul(a: &Mat4, b: &Mat4) -> Mat4 {
  let mut r = [0.0f32; 16];
  for col in 0..4 {
    for row in 0..4 {
      let mut sum = 0.0;
      for k in 0..4 {
        sum += a[k * 4 + row] * b[col * 4 + k];
      }
      r[col * 4 + row] = sum;
    }
  }
  r
}

fn transform_point(m: &Mat4, p: [f32; 3]) -> [f32; 4] {
  [
    m[0] * p[0] + m[4] * p[1] + m[8] * p[2] + m[12],
    m[1] * p[0] + m[5] * p[1] + m[9] * p[2] + m[13],
    m[2] * p[0] + m[6] * p[1] + m[10] * p[2] + m[14],
    m[3] * p[0] + m[7] * p[1] + m[11] * p[2] + m[15],
  ]
}

fn look_at(eye: [f32; 3], target: [f32; 3], up: [f32; 3]) -> Mat4 {
  let f = normalize(sub(target, eye));
  let s = normalize(cross(f, up));
  let u = cross(s, f);
  [
    s[0],
    u[0],
    -f[0],
    0.0,
    s[1],
    u[1],
    -f[1],
    0.0,
    s[2],
    u[2],
    -f[2],
    0.0,
    -(s[0] * eye[0] + s[1] * eye[1] + s[2] * eye[2]),
    -(u[0] * eye[0] + u[1] * eye[1] + u[2] * eye[2]),
    f[0] * eye[0] + f[1] * eye[1] + f[2] * eye[2],
    1.0,
  ]
}

fn ortho(
  left: f32,
  right: f32,
  bottom: f32,
  top: f32,
  near: f32,
  far: f32,
) -> Mat4 {
  let rl = right - left;
  let tb = top - bottom;
  let fmn = far - near;
  [
    2.0 / rl,
    0.0,
    0.0,
    0.0,
    0.0,
    2.0 / tb,
    0.0,
    0.0,
    0.0,
    0.0,
    -2.0 / fmn,
    0.0,
    -(right + left) / rl,
    -(top + bottom) / tb,
    -(far + near) / fmn,
    1.0,
  ]
}

// --- Rasterization with per-pixel Phong shading ---

fn rasterize_smooth_triangle(
  fb: &mut Framebuffer,
  screen: &[[f32; 3]; 3],
  normals: &[[f32; 3]; 3],
  color: [f32; 3],
  view_dir: &[f32; 3],
  lights: &[Light; 3],
) {
  let w = fb.width as f32;
  let h = fb.height as f32;

  // Bounding box clamped to framebuffer
  let min_x = screen[0][0].min(screen[1][0]).min(screen[2][0]).max(0.0);
  let max_x = screen[0][0]
    .max(screen[1][0])
    .max(screen[2][0])
    .min(w - 1.0);
  let min_y = screen[0][1].min(screen[1][1]).min(screen[2][1]).max(0.0);
  let max_y = screen[0][1]
    .max(screen[1][1])
    .max(screen[2][1])
    .min(h - 1.0);

  let v0 = screen[0];
  let v1 = screen[1];
  let v2 = screen[2];

  let area = edge_function(v0, v1, v2);
  if area.abs() < 1e-6 {
    return;
  }
  let inv_area = 1.0 / area;

  for y in (min_y as u32)..=(max_y as u32) {
    for x in (min_x as u32)..=(max_x as u32) {
      let px = x as f32 + 0.5;
      let py = y as f32 + 0.5;
      let p = [px, py, 0.0];

      let w0 = edge_function(v1, v2, p) * inv_area;
      let w1 = edge_function(v2, v0, p) * inv_area;
      let w2 = edge_function(v0, v1, p) * inv_area;

      if w0 >= 0.0 && w1 >= 0.0 && w2 >= 0.0 {
        let depth = w0 * v0[2] + w1 * v1[2] + w2 * v2[2];
        // Interpolate normal across triangle
        let pixel_normal =
          lerp3(normals[0], normals[1], normals[2], w0, w1, w2);
        let pixel_color = shade_pixel(pixel_normal, color, view_dir, lights);
        fb.set_pixel(x, y, depth, pixel_color);
      }
    }
  }
}

fn edge_function(a: [f32; 3], b: [f32; 3], c: [f32; 3]) -> f32 {
  (c[0] - a[0]) * (b[1] - a[1]) - (c[1] - a[1]) * (b[0] - a[0])
}

// --- PNG output ---

fn write_png(fb: &Framebuffer, path: &Path) -> Result<(), String> {
  let file = std::fs::File::create(path)
    .map_err(|e| format!("Failed to create {}: {e}", path.display()))?;
  let writer = BufWriter::new(file);

  let mut encoder = png::Encoder::new(writer, fb.width, fb.height);
  encoder.set_color(png::ColorType::Rgb);
  encoder.set_depth(png::BitDepth::Eight);

  let mut writer = encoder
    .write_header()
    .map_err(|e| format!("PNG header error: {e}"))?;

  let mut data = Vec::with_capacity((fb.width * fb.height * 3) as usize);
  for pixel in &fb.color {
    data.push(pixel[0]);
    data.push(pixel[1]);
    data.push(pixel[2]);
  }

  writer
    .write_image_data(&data)
    .map_err(|e| format!("PNG write error: {e}"))?;

  Ok(())
}
