//! Flatten a ScadNode CSG tree into groups of leaf primitives for OpenCSG rendering.
//!
//! OpenCSG renders a single "CSG product" at a time: a flat list of primitives each
//! tagged as Intersection or Subtraction. Complex CSG trees (nested unions, etc.) are
//! decomposed into multiple OpenCSG render calls (one per [`CsgGroup`]).

use luacad::export::{extract_manifold_mesh, materialize_scad_manifold};
use luacad::geometry::CsgGeometry;
use luacad::scad_export::{BoslPreviewParams, CylAxis, ScadNode};
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
    } else {
      #[cfg(feature = "csgrs")]
      if let Some(ref mesh) = geom.mesh {
        if !mesh.polygons.is_empty() {
          // Fallback: use the already-computed csgrs mesh as a single leaf.
          let vertices = cad_to_gl_vertices(mesh_to_triangles(mesh));
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

/// Returns true if `node` itself (not descendants) is a non-tessellatable
/// primitive that requires Manifold materialization. Used to decide whether a
/// boolean's immediate children need the whole boolean computed via Manifold
/// rather than OpenCSG, avoiding depth-buffer precision artifacts.
fn is_manifold_primitive(node: &ScadNode) -> bool {
  match node {
    ScadNode::Minkowski(_)
    | ScadNode::Hull(_)
    | ScadNode::LinearExtrude { .. }
    | ScadNode::RotateExtrude { .. } => true,
    // Look through transforms to the underlying shape
    ScadNode::Translate { child, .. }
    | ScadNode::Rotate { child, .. }
    | ScadNode::Scale { child, .. }
    | ScadNode::Mirror { child, .. }
    | ScadNode::Multmatrix { child, .. }
    | ScadNode::Resize { child, .. }
    | ScadNode::Color { child, .. }
    | ScadNode::Render { child, .. }
    | ScadNode::Modifier { child, .. } => is_manifold_primitive(child),
    _ => false,
  }
}

fn flatten_inner(node: &ScadNode, ctx: &Ctx, op: c_int) -> Vec<CsgGroup> {
  match node {
    // --- CSG booleans ---
    ScadNode::Union(children) => {
      // Each child of a union becomes its own group (separate OpenCSG render call).
      // Propagate `op` so that when a union appears inside a Difference (as a
      // subtracted operand), its leaves inherit the SUBTRACTION operation.
      let mut groups = Vec::new();
      for child in children {
        groups.extend(flatten_inner(child, ctx, op));
      }
      groups
    }
    ScadNode::Difference(children) if !children.is_empty() => {
      // If any child requires Manifold (Minkowski, Hull, extrusions), compute
      // the entire boolean via Manifold to avoid OpenCSG depth-buffer artifacts.
      if children.iter().any(is_manifold_primitive) {
        return manifold_preview(node, ctx, op, 1);
      }
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
      // If any child requires Manifold, compute via Manifold.
      if children.iter().any(is_manifold_primitive) {
        return manifold_preview(node, ctx, op, 1);
      }
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
    // Materialize via Manifold and render the resulting mesh as a leaf.
    ScadNode::LinearExtrude { .. }
    | ScadNode::RotateExtrude { .. }
    | ScadNode::Hull(_)
    | ScadNode::Minkowski(_) => manifold_preview(node, ctx, op, 1),

    // --- BOSL2 shapes with preview parameters ---
    ScadNode::BoslCall { preview, .. } => match preview {
      BoslPreviewParams::Cuboid {
        w,
        d,
        h,
        rounding,
        center,
      } => {
        if *rounding > 0.0 {
          let verts = manifold_rounded_cube(*w, *d, *h, *rounding, *center);
          make_leaf_group(verts, ctx, op, 1)
        } else {
          let verts = tessellate_cube(*w, *d, *h, *center);
          make_leaf_group(verts, ctx, op, 1)
        }
      }
      BoslPreviewParams::Cylinder {
        r1,
        r2,
        h,
        center,
        axis,
      } => {
        let verts = tessellate_cylinder(*r1, *r2, *h, 32, *center);
        match axis {
          CylAxis::Z => make_leaf_group(verts, ctx, op, 1),
          CylAxis::X => {
            let m = mat4_mul(&ctx.transform, &mat4_rotate_y(90.0));
            let child_ctx = Ctx {
              transform: m,
              color: ctx.color,
            };
            make_leaf_group(verts, &child_ctx, op, 1)
          }
          CylAxis::Y => {
            let m = mat4_mul(&ctx.transform, &mat4_rotate_x(-90.0));
            let child_ctx = Ctx {
              transform: m,
              color: ctx.color,
            };
            make_leaf_group(verts, &child_ctx, op, 1)
          }
        }
      }
      BoslPreviewParams::Sphere { r } => {
        let verts = tessellate_sphere(*r, 32);
        make_leaf_group(verts, ctx, op, 1)
      }
      BoslPreviewParams::Tube {
        or1,
        or2,
        ir1,
        ir2,
        h,
        center,
      } => {
        let outer = ScadNode::Cylinder {
          r1: *or1,
          r2: *or2,
          h: *h,
          segments: 32,
          center: *center,
        };
        let inner = ScadNode::Cylinder {
          r1: *ir1,
          r2: *ir2,
          h: *h,
          segments: 32,
          center: *center,
        };
        let node = ScadNode::Difference(vec![outer, inner]);
        manifold_preview(&node, ctx, op, 2)
      }
      BoslPreviewParams::Torus { r_maj, r_min } => {
        let node = torus_polyhedron(*r_maj, *r_min, 32, 16);
        manifold_preview(&node, ctx, op, 2)
      }
      BoslPreviewParams::Prismoid {
        size1,
        size2,
        h,
        center,
      } => {
        let node = prismoid_polyhedron(size1, size2, *h, *center);
        manifold_preview(&node, ctx, op, 1)
      }
      BoslPreviewParams::RectTube {
        size,
        isize,
        h,
        center,
      } => {
        let outer = ScadNode::Cube {
          w: size[0],
          d: size[1],
          h: *h,
          center: *center,
        };
        let inner = ScadNode::Cube {
          w: isize[0],
          d: isize[1],
          h: *h + 0.01, // slightly taller to ensure clean boolean
          center: *center,
        };
        let node = ScadNode::Difference(vec![outer, inner]);
        manifold_preview(&node, ctx, op, 2)
      }
      BoslPreviewParams::Wedge { w, d, h, center } => {
        let node = wedge_polyhedron(*w, *d, *h, *center);
        manifold_preview(&node, ctx, op, 1)
      }
      BoslPreviewParams::Octahedron { size } => {
        let node = octahedron_polyhedron(*size);
        manifold_preview(&node, ctx, op, 1)
      }
      BoslPreviewParams::PieSlice {
        r1,
        r2,
        h,
        ang,
        center,
      } => {
        let verts = tessellate_pie_slice(*r1, *r2, *h, *ang, *center);
        // Pie slice is non-convex: a ray through the center hits 2 front faces
        make_leaf_group(verts, ctx, op, 2)
      }
      BoslPreviewParams::RegularPrism {
        n,
        r1,
        r2,
        h,
        center,
      } => {
        // A regular prism is just a cylinder with segment count = n
        let verts = tessellate_cylinder(*r1, *r2, *h, *n, *center);
        make_leaf_group(verts, ctx, op, 1)
      }
      BoslPreviewParams::None => vec![],
    },

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

/// Compute a rounded cuboid mesh via Manifold's Minkowski sum of an inner
/// box with a sphere. Same approach OpenSCAD uses for Minkowski preview:
/// compute the full mesh first, then pass it to OpenCSG as a leaf primitive.
fn manifold_rounded_cube(
  w: f32,
  d: f32,
  h: f32,
  rounding: f32,
  center: bool,
) -> Vec<[f32; 3]> {
  let min_half = w.min(d).min(h) / 2.0;
  let r = rounding.min(min_half).max(0.0);
  if r < 1e-6 {
    return tessellate_cube(w, d, h, center);
  }

  // Build ScadNode tree: Minkowski(Cube(w-2r, d-2r, h-2r), Sphere(r))
  // This is the same decomposition BOSL2 uses internally.
  let inner = ScadNode::Cube {
    w: (w - 2.0 * r).max(0.001),
    d: (d - 2.0 * r).max(0.001),
    h: (h - 2.0 * r).max(0.001),
    center,
  };
  let ball = ScadNode::Sphere { r, segments: 32 };
  let node = if center {
    ScadNode::Minkowski(vec![inner, ball])
  } else {
    // Cube is corner-anchored, sphere is centered — shift result by +r
    ScadNode::Translate {
      x: r,
      y: r,
      z: r,
      child: Box::new(ScadNode::Minkowski(vec![inner, ball])),
    }
  };

  let manifold = materialize_scad_manifold(&node);
  let mesh = extract_manifold_mesh(&manifold);

  // Flatten indexed mesh to flat triangle vertex list
  let mut verts = Vec::with_capacity(mesh.triangles.len() * 3);
  for tri in &mesh.triangles {
    verts.push(mesh.vertices[tri[0] as usize]);
    verts.push(mesh.vertices[tri[1] as usize]);
    verts.push(mesh.vertices[tri[2] as usize]);
  }
  verts
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

// ---------------------------------------------------------------------------
// Manifold-based preview helpers for BOSL2 shapes
// ---------------------------------------------------------------------------

/// Build a ScadNode tree from primitives, materialize it via Manifold, and
/// return CsgGroups ready for OpenCSG rendering.
fn manifold_preview(
  node: &ScadNode,
  ctx: &Ctx,
  op: c_int,
  convexity: u32,
) -> Vec<CsgGroup> {
  let manifold = materialize_scad_manifold(node);
  let mesh = extract_manifold_mesh(&manifold);
  let mut verts = Vec::with_capacity(mesh.triangles.len() * 3);
  for tri in &mesh.triangles {
    verts.push(mesh.vertices[tri[0] as usize]);
    verts.push(mesh.vertices[tri[1] as usize]);
    verts.push(mesh.vertices[tri[2] as usize]);
  }
  make_leaf_group(verts, ctx, op, convexity)
}

/// Build a torus as a polyhedron (ring of circular cross-sections).
fn torus_polyhedron(
  r_maj: f32,
  r_min: f32,
  segs_maj: u32,
  segs_min: u32,
) -> ScadNode {
  let n_maj = segs_maj.max(3) as usize;
  let n_min = segs_min.max(3) as usize;

  let mut points = Vec::with_capacity(n_maj * n_min);
  for i in 0..n_maj {
    let theta = 2.0 * PI * i as f32 / n_maj as f32;
    let (st, ct) = (theta.sin(), theta.cos());
    for j in 0..n_min {
      let phi = 2.0 * PI * j as f32 / n_min as f32;
      let (sp, cp) = (phi.sin(), phi.cos());
      let x = (r_maj + r_min * cp) * ct;
      let y = (r_maj + r_min * cp) * st;
      let z = r_min * sp;
      points.push([x, y, z]);
    }
  }

  let mut faces = Vec::with_capacity(n_maj * n_min);
  for i in 0..n_maj {
    let i_next = (i + 1) % n_maj;
    for j in 0..n_min {
      let j_next = (j + 1) % n_min;
      // Quad as two triangles — but polyhedron supports quads via face lists
      faces.push(vec![
        i * n_min + j,
        i_next * n_min + j,
        i_next * n_min + j_next,
        i * n_min + j_next,
      ]);
    }
  }

  ScadNode::Polyhedron { points, faces }
}

/// Build a prismoid (rectangular frustum) as a polyhedron.
fn prismoid_polyhedron(
  size1: &[f32; 2],
  size2: &[f32; 2],
  h: f32,
  center: bool,
) -> ScadNode {
  let z_off = if center { -h / 2.0 } else { 0.0 };
  let (hw1, hd1) = (size1[0] / 2.0, size1[1] / 2.0);
  let (hw2, hd2) = (size2[0] / 2.0, size2[1] / 2.0);

  let points = vec![
    // Bottom face (z = z_off)
    [-hw1, -hd1, z_off],
    [hw1, -hd1, z_off],
    [hw1, hd1, z_off],
    [-hw1, hd1, z_off],
    // Top face (z = z_off + h)
    [-hw2, -hd2, z_off + h],
    [hw2, -hd2, z_off + h],
    [hw2, hd2, z_off + h],
    [-hw2, hd2, z_off + h],
  ];

  let faces = vec![
    vec![3, 2, 1, 0], // bottom (CCW from below)
    vec![4, 5, 6, 7], // top
    vec![0, 1, 5, 4], // front
    vec![2, 3, 7, 6], // back
    vec![0, 4, 7, 3], // left
    vec![1, 2, 6, 5], // right
  ];

  ScadNode::Polyhedron { points, faces }
}

/// Build a wedge (triangular prism) as a polyhedron.
/// The wedge has its right-angle at the bottom-left:
///   bottom face is a full rectangle, top face tapers to a line along the left edge.
fn wedge_polyhedron(w: f32, d: f32, h: f32, center: bool) -> ScadNode {
  let (ox, oy, oz) = if center {
    (-w / 2.0, -d / 2.0, -h / 2.0)
  } else {
    (0.0, 0.0, 0.0)
  };

  let points = vec![
    [ox, oy, oz],         // 0: bottom-front-left
    [ox + w, oy, oz],     // 1: bottom-front-right
    [ox + w, oy + d, oz], // 2: bottom-back-right
    [ox, oy + d, oz],     // 3: bottom-back-left
    [ox, oy, oz + h],     // 4: top-front-left
    [ox, oy + d, oz + h], // 5: top-back-left
  ];

  let faces = vec![
    vec![3, 2, 1, 0], // bottom
    vec![4, 5, 3, 0], // left
    vec![0, 1, 4],    // front (triangle)
    vec![2, 3, 5],    // back (triangle)
    vec![1, 2, 5, 4], // slope
  ];

  ScadNode::Polyhedron { points, faces }
}

/// Build an octahedron as a polyhedron.
fn octahedron_polyhedron(size: f32) -> ScadNode {
  let s = size / 2.0;

  let points = vec![
    [s, 0.0, 0.0],  // 0: +X
    [-s, 0.0, 0.0], // 1: -X
    [0.0, s, 0.0],  // 2: +Y
    [0.0, -s, 0.0], // 3: -Y
    [0.0, 0.0, s],  // 4: +Z
    [0.0, 0.0, -s], // 5: -Z
  ];

  let faces = vec![
    vec![0, 2, 4], // +X +Y +Z
    vec![2, 1, 4], // -X +Y +Z
    vec![1, 3, 4], // -X -Y +Z
    vec![3, 0, 4], // +X -Y +Z
    vec![2, 0, 5], // +X +Y -Z
    vec![1, 2, 5], // -X +Y -Z
    vec![3, 1, 5], // -X -Y -Z
    vec![0, 3, 5], // +X -Y -Z
  ];

  ScadNode::Polyhedron { points, faces }
}

/// Tessellate a pie slice (partial cylinder) directly as triangles.
fn tessellate_pie_slice(
  r1: f32,
  r2: f32,
  h: f32,
  ang_deg: f32,
  center: bool,
) -> Vec<[f32; 3]> {
  let ang = ang_deg.clamp(0.0, 360.0).to_radians();
  let segs = ((ang_deg / 360.0 * 32.0).ceil() as u32).max(1) as usize;
  let z_off = if center { -h / 2.0 } else { 0.0 };

  let mut verts = Vec::new();
  let center_b = [0.0, 0.0, z_off];
  let center_t = [0.0, 0.0, z_off + h];

  for i in 0..segs {
    let a0 = ang * i as f32 / segs as f32;
    let a1 = ang * (i + 1) as f32 / segs as f32;
    let (s0, c0) = (a0.sin(), a0.cos());
    let (s1, c1) = (a1.sin(), a1.cos());

    let b0 = [r1 * c0, r1 * s0, z_off];
    let b1 = [r1 * c1, r1 * s1, z_off];
    let t0 = [r2 * c0, r2 * s0, z_off + h];
    let t1 = [r2 * c1, r2 * s1, z_off + h];

    // Curved side (two triangles per segment)
    verts.push(b0);
    verts.push(b1);
    verts.push(t1);
    verts.push(b0);
    verts.push(t1);
    verts.push(t0);

    // Bottom cap (fan from center)
    verts.push(center_b);
    verts.push(b1);
    verts.push(b0);

    // Top cap (fan from center)
    verts.push(center_t);
    verts.push(t0);
    verts.push(t1);
  }

  // Start flat side (at angle=0): quad center_b→bs→ts→center_t
  // Normal should point -Y (away from pie interior)
  let bs = [r1, 0.0, z_off];
  let ts = [r2, 0.0, z_off + h];
  // Triangle 1: center_b, bs, ts  (matches cylinder side winding convention)
  verts.push(center_b);
  verts.push(bs);
  verts.push(ts);
  // Triangle 2: center_b, ts, center_t
  verts.push(center_b);
  verts.push(ts);
  verts.push(center_t);

  // End flat side (at angle=ang): quad center_b→center_t→te→be
  // Normal should point outward (rotated +Y direction)
  let (se, ce) = (ang.sin(), ang.cos());
  let be = [r1 * ce, r1 * se, z_off];
  let te = [r2 * ce, r2 * se, z_off + h];
  // Triangle 1: center_b, te, be  (reversed from start side)
  verts.push(center_b);
  verts.push(te);
  verts.push(be);
  // Triangle 2: center_b, center_t, te
  verts.push(center_b);
  verts.push(center_t);
  verts.push(te);

  verts
}

/// Convert a csgrs mesh into flat triangle vertices (CAD coordinates).
#[cfg(feature = "csgrs")]
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
