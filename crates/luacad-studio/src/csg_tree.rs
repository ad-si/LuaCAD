//! Flatten a ScadNode CSG tree into groups of leaf primitives for OpenCSG rendering.
//!
//! OpenCSG renders a single "CSG product" at a time: a flat list of primitives each
//! tagged as Intersection or Subtraction. Complex CSG trees (nested unions, etc.) are
//! decomposed into multiple OpenCSG render calls (one per [`CsgGroup`]).

use luacad::geometry::CsgGeometry;
use luacad::scad_export::ScadNode;
use opencsg_sys::{INTERSECTION, SUBTRACTION};
use std::f32::consts::PI;
use std::os::raw::c_int;

/// A single leaf primitive ready for OpenCSG rendering.
pub struct CsgLeaf {
  /// Triangle vertices (groups of 3 positions). GL coordinates (Y-up).
  pub vertices: Vec<[f32; 3]>,
  /// Accumulated model-to-world transform (column-major 4x4).
  pub transform: [f32; 16],
  /// OpenCSG operation: INTERSECTION or SUBTRACTION.
  pub operation: c_int,
  /// Convexity (max front faces at a single point). 1 for convex shapes.
  pub convexity: u32,
  /// Per-primitive color (RGB, 0..1).
  pub color: [f32; 3],
}

/// A group of primitives that form a single OpenCSG render call.
pub struct CsgGroup {
  pub primitives: Vec<CsgLeaf>,
}

/// Default color when none is specified.
const DEFAULT_COLOR: [f32; 3] = [0.192, 0.467, 0.745]; // #3177be

// --- Public API ---

/// Flatten all geometries' ScadNode trees into CsgGroups for OpenCSG.
/// Falls back to using the csgrs mesh when no ScadNode is available.
pub fn flatten_geometries(geometries: &[CsgGeometry]) -> Vec<CsgGroup> {
  let mut groups = Vec::new();
  for geom in geometries {
    let color = geom.color.unwrap_or(DEFAULT_COLOR);
    if let Some(ref scad) = geom.scad {
      groups.extend(flatten_node(scad, &IDENTITY, color));
    } else if !geom.mesh.polygons.is_empty() {
      // Fallback: use the already-computed csgrs mesh as a single leaf.
      let vertices = cad_to_gl_vertices(mesh_to_triangles(&geom.mesh));
      if !vertices.is_empty() {
        groups.push(CsgGroup {
          primitives: vec![CsgLeaf {
            vertices,
            transform: IDENTITY,
            operation: INTERSECTION,
            convexity: 1,
            color,
          }],
        });
      }
    }
  }
  groups
}

// --- Matrix helpers ---

const IDENTITY: [f32; 16] = [
  1.0, 0.0, 0.0, 0.0, //
  0.0, 1.0, 0.0, 0.0, //
  0.0, 0.0, 1.0, 0.0, //
  0.0, 0.0, 0.0, 1.0, //
];

/// Multiply two column-major 4x4 matrices: result = a * b.
fn mat4_mul(a: &[f32; 16], b: &[f32; 16]) -> [f32; 16] {
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

fn mat4_translate(x: f32, y: f32, z: f32) -> [f32; 16] {
  [
    1.0, 0.0, 0.0, 0.0, //
    0.0, 1.0, 0.0, 0.0, //
    0.0, 0.0, 1.0, 0.0, //
    x, y, z, 1.0, //
  ]
}

fn mat4_scale(x: f32, y: f32, z: f32) -> [f32; 16] {
  [
    x, 0.0, 0.0, 0.0, //
    0.0, y, 0.0, 0.0, //
    0.0, 0.0, z, 0.0, //
    0.0, 0.0, 0.0, 1.0, //
  ]
}

fn mat4_rotate_x(deg: f32) -> [f32; 16] {
  let r = deg.to_radians();
  let (s, c) = (r.sin(), r.cos());
  [
    1.0, 0.0, 0.0, 0.0, //
    0.0, c, s, 0.0, //
    0.0, -s, c, 0.0, //
    0.0, 0.0, 0.0, 1.0, //
  ]
}

fn mat4_rotate_y(deg: f32) -> [f32; 16] {
  let r = deg.to_radians();
  let (s, c) = (r.sin(), r.cos());
  [
    c, 0.0, -s, 0.0, //
    0.0, 1.0, 0.0, 0.0, //
    s, 0.0, c, 0.0, //
    0.0, 0.0, 0.0, 1.0, //
  ]
}

fn mat4_rotate_z(deg: f32) -> [f32; 16] {
  let r = deg.to_radians();
  let (s, c) = (r.sin(), r.cos());
  [
    c, s, 0.0, 0.0, //
    -s, c, 0.0, 0.0, //
    0.0, 0.0, 1.0, 0.0, //
    0.0, 0.0, 0.0, 1.0, //
  ]
}

fn mat4_mirror(nx: f32, ny: f32, nz: f32) -> [f32; 16] {
  // Householder reflection: I - 2 * n * n^T (assuming n is normalized)
  let len = (nx * nx + ny * ny + nz * nz).sqrt();
  if len < 1e-12 {
    return IDENTITY;
  }
  let (nx, ny, nz) = (nx / len, ny / len, nz / len);
  [
    1.0 - 2.0 * nx * nx,
    -2.0 * nx * ny,
    -2.0 * nx * nz,
    0.0,
    -2.0 * ny * nx,
    1.0 - 2.0 * ny * ny,
    -2.0 * ny * nz,
    0.0,
    -2.0 * nz * nx,
    -2.0 * nz * ny,
    1.0 - 2.0 * nz * nz,
    0.0,
    0.0,
    0.0,
    0.0,
    1.0,
  ]
}

// --- Tree flattening ---

/// Context passed down while recursing through the ScadNode tree.
struct Ctx {
  transform: [f32; 16],
  color: [f32; 3],
}

fn flatten_node(
  node: &ScadNode,
  parent_xform: &[f32; 16],
  color: [f32; 3],
) -> Vec<CsgGroup> {
  let ctx = Ctx {
    transform: *parent_xform,
    color,
  };
  flatten_inner(node, &ctx, INTERSECTION)
}

fn flatten_inner(node: &ScadNode, ctx: &Ctx, op: c_int) -> Vec<CsgGroup> {
  match node {
    // --- CSG booleans ---
    ScadNode::Union(children) => {
      // Each child of a union becomes its own group (separate OpenCSG render call).
      let mut groups = Vec::new();
      for child in children {
        groups.extend(flatten_inner(child, ctx, INTERSECTION));
      }
      groups
    }
    ScadNode::Difference(children) if !children.is_empty() => {
      // First child = Intersection, rest = Subtraction, all in one group.
      let mut leaves = Vec::new();
      for (i, child) in children.iter().enumerate() {
        let child_op = if i == 0 { INTERSECTION } else { SUBTRACTION };
        let child_groups = flatten_inner(child, ctx, child_op);
        for g in child_groups {
          leaves.extend(g.primitives);
        }
      }
      if leaves.is_empty() {
        vec![]
      } else {
        vec![CsgGroup { primitives: leaves }]
      }
    }
    ScadNode::Intersection(children) => {
      // All children are Intersection, in one group.
      let mut leaves = Vec::new();
      for child in children {
        let child_groups = flatten_inner(child, ctx, INTERSECTION);
        for g in child_groups {
          leaves.extend(g.primitives);
        }
      }
      if leaves.is_empty() {
        vec![]
      } else {
        vec![CsgGroup { primitives: leaves }]
      }
    }

    // --- Transforms ---
    ScadNode::Translate { x, y, z, child } => {
      let m = mat4_mul(&ctx.transform, &mat4_translate(*x, *y, *z));
      let child_ctx = Ctx {
        transform: m,
        color: ctx.color,
      };
      flatten_inner(child, &child_ctx, op)
    }
    ScadNode::Rotate { x, y, z, child } => {
      // OpenSCAD rotation order: Z then Y then X
      let m = mat4_mul(&ctx.transform, &mat4_rotate_z(*z));
      let m = mat4_mul(&m, &mat4_rotate_y(*y));
      let m = mat4_mul(&m, &mat4_rotate_x(*x));
      let child_ctx = Ctx {
        transform: m,
        color: ctx.color,
      };
      flatten_inner(child, &child_ctx, op)
    }
    ScadNode::Scale { x, y, z, child } => {
      let m = mat4_mul(&ctx.transform, &mat4_scale(*x, *y, *z));
      let child_ctx = Ctx {
        transform: m,
        color: ctx.color,
      };
      flatten_inner(child, &child_ctx, op)
    }
    ScadNode::Mirror { x, y, z, child } => {
      let m = mat4_mul(&ctx.transform, &mat4_mirror(*x, *y, *z));
      let child_ctx = Ctx {
        transform: m,
        color: ctx.color,
      };
      flatten_inner(child, &child_ctx, op)
    }
    ScadNode::Multmatrix { matrix, child } => {
      // ScadNode stores row-major; OpenGL wants column-major. Transpose.
      let col_major = row_to_col_major(matrix);
      let m = mat4_mul(&ctx.transform, &col_major);
      let child_ctx = Ctx {
        transform: m,
        color: ctx.color,
      };
      flatten_inner(child, &child_ctx, op)
    }
    ScadNode::Resize { child, .. } => {
      // Resize is hard to decompose — just pass through with current transform.
      flatten_inner(child, ctx, op)
    }
    ScadNode::Color { r, g, b, child, .. } => {
      let child_ctx = Ctx {
        transform: ctx.transform,
        color: [*r, *g, *b],
      };
      flatten_inner(child, &child_ctx, op)
    }
    ScadNode::Render { child, .. } => flatten_inner(child, ctx, op),
    ScadNode::Modifier { child, .. } => flatten_inner(child, ctx, op),

    // --- Leaf 3D primitives ---
    ScadNode::Cube { w, d, h, center } => {
      let verts = tessellate_cube(*w, *d, *h, *center);
      make_leaf_group(verts, ctx, op, 1)
    }
    ScadNode::Sphere { r, segments } => {
      let verts = tessellate_sphere(*r, *segments);
      make_leaf_group(verts, ctx, op, 1)
    }
    ScadNode::Cylinder {
      r1,
      r2,
      h,
      segments,
      center,
    } => {
      let verts = tessellate_cylinder(*r1, *r2, *h, *segments, *center);
      make_leaf_group(verts, ctx, op, 1)
    }
    ScadNode::Polyhedron { points, faces } => {
      let verts = tessellate_polyhedron(points, faces);
      make_leaf_group(verts, ctx, op, 1)
    }

    // --- Extrusions / Hull / Minkowski: not directly tessellatable ---
    // These produce complex geometry that can't be trivially tessellated from
    // the ScadNode parameters alone. We'd need the csgrs mesh. Since we don't
    // have direct access to it here, produce an empty group (these will fall
    // back to the mesh-based path in flatten_geometries).
    ScadNode::LinearExtrude { .. }
    | ScadNode::RotateExtrude { .. }
    | ScadNode::Hull(_)
    | ScadNode::Minkowski(_) => vec![],

    // --- 2D primitives, file ops, text, etc.: no 3D geometry ---
    _ => vec![],
  }
}

fn row_to_col_major(row: &[f32; 16]) -> [f32; 16] {
  [
    row[0], row[4], row[8], row[12], //
    row[1], row[5], row[9], row[13], //
    row[2], row[6], row[10], row[14], //
    row[3], row[7], row[11], row[15], //
  ]
}

/// Convert vertices from CAD space (Z-up) to GL space (Y-up).
/// Mapping: CAD (x,y,z) → GL (y,z,x).
fn cad_to_gl_vertices(verts: Vec<[f32; 3]>) -> Vec<[f32; 3]> {
  verts.into_iter().map(|[x, y, z]| [y, z, x]).collect()
}

fn make_leaf_group(
  vertices: Vec<[f32; 3]>,
  ctx: &Ctx,
  op: c_int,
  convexity: u32,
) -> Vec<CsgGroup> {
  if vertices.is_empty() {
    return vec![];
  }
  vec![CsgGroup {
    primitives: vec![CsgLeaf {
      vertices: cad_to_gl_vertices(vertices),
      transform: ctx.transform,
      operation: op,
      convexity,
      color: ctx.color,
    }],
  }]
}

// --- Tessellation ---

fn tessellate_cube(w: f32, d: f32, h: f32, center: bool) -> Vec<[f32; 3]> {
  let (ox, oy, oz) = if center {
    (-w / 2.0, -d / 2.0, -h / 2.0)
  } else {
    (0.0, 0.0, 0.0)
  };
  // 8 corners
  let v = [
    [ox, oy, oz],
    [ox + w, oy, oz],
    [ox + w, oy + d, oz],
    [ox, oy + d, oz],
    [ox, oy, oz + h],
    [ox + w, oy, oz + h],
    [ox + w, oy + d, oz + h],
    [ox, oy + d, oz + h],
  ];
  // 6 faces, 2 triangles each, CCW winding from outside
  let faces: [[usize; 3]; 12] = [
    // bottom (z=oz)
    [0, 2, 1],
    [0, 3, 2],
    // top (z=oz+h)
    [4, 5, 6],
    [4, 6, 7],
    // front (y=oy)
    [0, 1, 5],
    [0, 5, 4],
    // back (y=oy+d)
    [2, 3, 7],
    [2, 7, 6],
    // left (x=ox)
    [0, 4, 7],
    [0, 7, 3],
    // right (x=ox+w)
    [1, 2, 6],
    [1, 6, 5],
  ];
  let mut out = Vec::with_capacity(36);
  for f in &faces {
    out.push(v[f[0]]);
    out.push(v[f[1]]);
    out.push(v[f[2]]);
  }
  out
}

fn tessellate_sphere(r: f32, segments: u32) -> Vec<[f32; 3]> {
  let segs = segments.max(4) as usize;
  let rings = segs / 2; // latitude divisions

  let mut verts = Vec::new();
  for j in 0..rings {
    let theta0 = PI * j as f32 / rings as f32;
    let theta1 = PI * (j + 1) as f32 / rings as f32;
    let (s0, c0) = (theta0.sin(), theta0.cos());
    let (s1, c1) = (theta1.sin(), theta1.cos());
    for i in 0..segs {
      let phi0 = 2.0 * PI * i as f32 / segs as f32;
      let phi1 = 2.0 * PI * (i + 1) as f32 / segs as f32;
      let (sp0, cp0) = (phi0.sin(), phi0.cos());
      let (sp1, cp1) = (phi1.sin(), phi1.cos());

      let p00 = [r * s0 * cp0, r * s0 * sp0, r * c0];
      let p10 = [r * s1 * cp0, r * s1 * sp0, r * c1];
      let p01 = [r * s0 * cp1, r * s0 * sp1, r * c0];
      let p11 = [r * s1 * cp1, r * s1 * sp1, r * c1];

      // Two triangles per quad (skip degenerate triangles at poles)
      if j > 0 {
        verts.push(p00);
        verts.push(p10);
        verts.push(p01);
      }
      if j < rings - 1 {
        verts.push(p01);
        verts.push(p10);
        verts.push(p11);
      }
    }
  }
  verts
}

fn tessellate_cylinder(
  r1: f32,
  r2: f32,
  h: f32,
  segments: u32,
  center: bool,
) -> Vec<[f32; 3]> {
  let segs = segments.max(3) as usize;
  let z_off = if center { -h / 2.0 } else { 0.0 };

  let mut verts = Vec::new();

  for i in 0..segs {
    let a0 = 2.0 * PI * i as f32 / segs as f32;
    let a1 = 2.0 * PI * (i + 1) as f32 / segs as f32;
    let (s0, c0) = (a0.sin(), a0.cos());
    let (s1, c1) = (a1.sin(), a1.cos());

    let b0 = [r1 * c0, r1 * s0, z_off];
    let b1 = [r1 * c1, r1 * s1, z_off];
    let t0 = [r2 * c0, r2 * s0, z_off + h];
    let t1 = [r2 * c1, r2 * s1, z_off + h];

    // Side face (two triangles)
    verts.push(b0);
    verts.push(b1);
    verts.push(t1);
    verts.push(b0);
    verts.push(t1);
    verts.push(t0);

    // Bottom cap (fan from center)
    if r1 > 0.0 {
      let center_b = [0.0, 0.0, z_off];
      verts.push(center_b);
      verts.push(b1);
      verts.push(b0);
    }

    // Top cap (fan from center)
    if r2 > 0.0 {
      let center_t = [0.0, 0.0, z_off + h];
      verts.push(center_t);
      verts.push(t0);
      verts.push(t1);
    }
  }
  verts
}

fn tessellate_polyhedron(
  points: &[[f32; 3]],
  faces: &[Vec<usize>],
) -> Vec<[f32; 3]> {
  let mut verts = Vec::new();
  for face in faces {
    if face.len() < 3 {
      continue;
    }
    // Fan-triangulate
    let v0 = points[face[0]];
    for i in 1..face.len() - 1 {
      verts.push(v0);
      verts.push(points[face[i]]);
      verts.push(points[face[i + 1]]);
    }
  }
  verts
}

/// Convert a csgrs mesh into flat triangle vertices (CAD coordinates).
fn mesh_to_triangles(mesh: &csgrs::mesh::Mesh<()>) -> Vec<[f32; 3]> {
  let tri = mesh.triangulate();
  let mut verts = Vec::new();
  for poly in &tri.polygons {
    let base_verts: Vec<[f32; 3]> = poly
      .vertices
      .iter()
      .map(|v| [v.pos.x, v.pos.y, v.pos.z])
      .collect();
    // Fan-triangulate
    for i in 1..base_verts.len().saturating_sub(1) {
      verts.push(base_verts[0]);
      verts.push(base_verts[i]);
      verts.push(base_verts[i + 1]);
    }
  }
  verts
}
