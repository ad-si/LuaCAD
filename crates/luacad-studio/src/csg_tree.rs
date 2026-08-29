//! Flatten a ScadNode CSG tree into groups of leaf primitives for OpenCSG rendering.
//!
//! OpenCSG renders a single "CSG product" at a time: a flat list of primitives each
//! tagged as Intersection or Subtraction. Complex CSG trees (nested unions, etc.) are
//! decomposed into multiple OpenCSG render calls (one per [`CsgGroup`]).

use luacad::export::{
  Dimension, extract_manifold_mesh, materialize_scad_display_mesh,
  materialize_scad_manifold, node_dimension,
};
use luacad::geometry::CsgGeometry;
use luacad::material::MaterialSpec;
use luacad::scad_export::{BoslPreviewParams, CylAxis, ModifierKind, ScadNode};
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
  /// Per-primitive surface material (approximated by fixed-function GL).
  pub material: MaterialSpec,
}

/// A group of primitives that form a single OpenCSG render call.
pub struct CsgGroup {
  pub primitives: Vec<CsgLeaf>,
}

/// A translucent mesh drawn over the CSG result in a blended pass:
/// `#` (debug highlight) and `%` (background) modifier geometry.
pub struct OverlayMesh {
  /// Triangle vertices (groups of 3 positions). GL coordinates (Y-up).
  pub vertices: Vec<[f32; 3]>,
  /// Accumulated model-to-world transform (column-major 4x4, CAD space).
  pub transform: [f32; 16],
  /// RGBA color, 0..1.
  pub color: [f32; 4],
}

/// The materialized surface of one colored part of the model: the boolean
/// result rather than the CSG inputs, so it carries the surfaces inside a
/// part (bore walls, enclosed cavities) that the preview's front-most-surface
/// CSG never produces. Used by the transparent view mode.
pub struct SolidMesh {
  /// Triangle vertices (groups of 3 positions). GL coordinates (Y-up), with
  /// every transform already applied.
  pub vertices: Vec<[f32; 3]>,
  /// Color (RGB, 0..1).
  pub color: [f32; 3],
  /// Surface material (approximated by fixed-function GL).
  pub material: MaterialSpec,
}

/// Everything needed to draw the preview: opaque CSG groups plus
/// translucent modifier overlays.
#[derive(Default)]
pub struct CsgScene {
  pub groups: Vec<CsgGroup>,
  pub overlays: Vec<OverlayMesh>,
}

/// Side effects of OpenSCAD modifiers collected while flattening:
/// `#`/`%` overlays and the first `!` (show-only) subtree.
#[derive(Default)]
struct ModifierSink {
  overlays: Vec<OverlayMesh>,
  only: Option<CsgScene>,
}

/// Default color when none is specified.
const DEFAULT_COLOR: [f32; 3] = [0.192, 0.467, 0.745]; // #3177be

/// Translucent red for `#` (debug/highlight), matching OpenSCAD.
const HIGHLIGHT_COLOR: [f32; 4] = [1.0, 0.32, 0.32, 0.5];

/// Translucent gray for `%` (background/transparent), matching OpenSCAD.
const BACKGROUND_COLOR: [f32; 4] = [0.71, 0.71, 0.71, 0.5];

// --- Public API ---

/// Flatten all geometries' ScadNode trees into a CsgScene for OpenCSG.
/// Falls back to using the csgrs mesh when no ScadNode is available.
pub fn flatten_geometries(geometries: &[CsgGeometry]) -> CsgScene {
  let mut sink = ModifierSink::default();
  let mut groups = Vec::new();
  for geom in geometries {
    let material = geom.material.unwrap_or_default();
    if let Some(ref scad) = geom.scad {
      groups.extend(flatten_node(
        scad, &IDENTITY, geom.color, material, &mut sink,
      ));
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
                color: geom
                  .color
                  .or(material.default_color)
                  .unwrap_or(DEFAULT_COLOR),
                material,
              }],
            });
          }
        }
      }
    }
  }
  match sink.only {
    // `!` replaces the whole scene with the marked subtree.
    Some(only_scene) => only_scene,
    None => CsgScene {
      groups,
      overlays: sink.overlays,
    },
  }
}

/// Materialize the geometries into solid meshes for the transparent view.
///
/// Expensive (a Manifold boolean per colored part), so this belongs on the
/// background execution thread next to [`flatten_geometries`], never in the
/// render loop.
pub fn solid_meshes(geometries: &[CsgGeometry]) -> Vec<SolidMesh> {
  luacad::render::display_solids(geometries)
    .into_iter()
    .map(|solid| SolidMesh {
      vertices: cad_to_gl_vertices(solid.vertices),
      color: solid.color,
      material: solid.material,
    })
    .collect()
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

/// Determinant of the upper-left 3x3 of a column-major 4x4: negative
/// exactly when the transform reflects (an odd number of mirrors or
/// negative scale axes).
fn mat4_det3(m: &[f32; 16]) -> f32 {
  m[0] * (m[5] * m[10] - m[6] * m[9])
    + m[1] * (m[6] * m[8] - m[4] * m[10])
    + m[2] * (m[4] * m[9] - m[5] * m[8])
}

/// Apply a column-major 4x4 to a point.
fn mat4_apply_point(m: &[f32; 16], p: [f32; 3]) -> [f32; 3] {
  let [x, y, z] = p;
  [
    m[0] * x + m[4] * y + m[8] * z + m[12],
    m[1] * x + m[5] * y + m[9] * z + m[13],
    m[2] * x + m[6] * y + m[10] * z + m[14],
  ]
}

/// A reflecting transform turns the triangles it maps inside out: the
/// winding flips, so back-face culling drops the surfaces and the part
/// shows up only in transparent mode (the export and raytrace paths
/// re-orient their meshes and were never affected). The face normals
/// are computed from the vertex order in leaf space, so flipping GL's
/// front-face state alone would light the surfaces from the inside -
/// instead the transform is baked into the vertices and every triangle
/// reversed, which restores the winding and the outward normals in
/// one go.
fn bake_reflection(
  mut vertices: Vec<[f32; 3]>,
  transform: &[f32; 16],
) -> (Vec<[f32; 3]>, [f32; 16]) {
  if mat4_det3(transform) >= 0.0 {
    return (vertices, *transform);
  }
  for v in &mut vertices {
    *v = mat4_apply_point(transform, *v);
  }
  for tri in vertices.as_chunks_mut::<3>().0 {
    tri.swap(1, 2);
  }
  (vertices, IDENTITY)
}

// --- Tree flattening ---

/// Context passed down while recursing through the ScadNode tree.
#[derive(Clone, Copy)]
struct Ctx {
  transform: [f32; 16],
  /// Color set by an explicit `Color` node during the walk.
  color: Option<[f32; 3]>,
  /// The geometry's struct-level color: the base a boolean result inherits
  /// from its left operand. Weaker than a material's default color, so
  /// presets like "gold" keep their look inside an uncolored union.
  base_color: Option<[f32; 3]>,
  material: MaterialSpec,
  /// Depth complexity promised by an enclosing `Render` node: how many
  /// front-facing surfaces a ray may cross. OpenCSG's depth pass peels
  /// only this many layers, so a subtracted concave primitive (e.g. a
  /// thread) previewed with too small a value loses its deeper surfaces.
  convexity: u32,
}

impl Ctx {
  /// The color a leaf under this context draws with:
  /// explicit color > material default color > inherited base > default blue.
  fn resolved_color(&self) -> [f32; 3] {
    self
      .color
      .or(self.material.default_color)
      .or(self.base_color)
      .unwrap_or(DEFAULT_COLOR)
  }
}

fn flatten_node(
  node: &ScadNode,
  parent_xform: &[f32; 16],
  base_color: Option<[f32; 3]>,
  material: MaterialSpec,
  sink: &mut ModifierSink,
) -> Vec<CsgGroup> {
  let ctx = Ctx {
    transform: *parent_xform,
    color: None,
    base_color,
    material,
    convexity: 1,
  };
  flatten_inner(node, &ctx, INTERSECTION, sink)
}

type Aabb = ([f32; 3], [f32; 3]);

fn bbox_of_points<'a>(points: impl Iterator<Item = &'a [f32; 3]>) -> Option<Aabb> {
  let mut min = [f32::INFINITY; 3];
  let mut max = [f32::NEG_INFINITY; 3];
  let mut any = false;
  for p in points {
    any = true;
    for axis in 0..3 {
      min[axis] = min[axis].min(p[axis]);
      max[axis] = max[axis].max(p[axis]);
    }
  }
  any.then_some((min, max))
}

fn bbox_union(a: Aabb, b: Aabb) -> Aabb {
  let mut min = a.0;
  let mut max = a.1;
  for axis in 0..3 {
    min[axis] = min[axis].min(b.0[axis]);
    max[axis] = max[axis].max(b.1[axis]);
  }
  (min, max)
}

fn bbox_intersection(a: Aabb, b: Aabb) -> Aabb {
  let mut min = a.0;
  let mut max = a.1;
  for axis in 0..3 {
    min[axis] = min[axis].max(b.0[axis]);
    max[axis] = max[axis].min(b.1[axis]);
    // An empty overlap collapses to a point rather than an inverted box.
    max[axis] = max[axis].max(min[axis]);
  }
  (min, max)
}

/// The box's corners pushed through `m` (column-major), re-boxed. Grows under
/// rotation; conservative is all the callers need.
fn transform_bbox(m: &[f32; 16], (min, max): Aabb) -> Aabb {
  let mut corners = [[0.0f32; 3]; 8];
  for (i, corner) in corners.iter_mut().enumerate() {
    *corner = mat4_apply_point(
      m,
      [
        if i & 1 == 0 { min[0] } else { max[0] },
        if i & 2 == 0 { min[1] } else { max[1] },
        if i & 4 == 0 { min[2] } else { max[2] },
      ],
    );
  }
  bbox_of_points(corners.iter()).unwrap()
}

/// Conservative axis-aligned bounding box of a subtree, in the subtree's own
/// coordinate space, computed without materializing anything. `None` means
/// unknown (a file import, text, raw SCAD, a BOSL call) and the caller must
/// assume nothing about the extent.
fn node_bbox(node: &ScadNode) -> Option<Aabb> {
  match node {
    ScadNode::Cube { w, d, h, center } => Some(if *center {
      ([-w / 2.0, -d / 2.0, -h / 2.0], [w / 2.0, d / 2.0, h / 2.0])
    } else {
      ([0.0; 3], [*w, *d, *h])
    }),
    ScadNode::Sphere { r, .. } => Some(([-r, -r, -r], [*r, *r, *r])),
    ScadNode::Cylinder {
      r1, r2, h, center, ..
    } => {
      let r = r1.max(*r2);
      let (z0, z1) = if *center { (-h / 2.0, h / 2.0) } else { (0.0, *h) };
      Some(([-r, -r, z0], [r, r, z1]))
    }
    ScadNode::Polyhedron { points, .. } => bbox_of_points(points.iter()),

    ScadNode::Circle { r, .. } => Some(([-r, -r, 0.0], [*r, *r, 0.0])),
    ScadNode::Square { w, h, center } => Some(if *center {
      ([-w / 2.0, -h / 2.0, 0.0], [w / 2.0, h / 2.0, 0.0])
    } else {
      ([0.0; 3], [*w, *h, 0.0])
    }),
    ScadNode::Polygon { points, .. } => {
      let points: Vec<[f32; 3]> =
        points.iter().map(|p| [p[0], p[1], 0.0]).collect();
      bbox_of_points(points.iter())
    }

    ScadNode::LinearExtrude {
      height,
      center,
      twist,
      scale,
      child,
      ..
    } => {
      let (min, max) = node_bbox(child)?;
      let (z0, z1) = if *center {
        (-height / 2.0, height / 2.0)
      } else {
        (0.0, *height)
      };
      // A twisted profile sweeps a circle; an untwisted one only grows by
      // the top scale. Either way a radius bound around the axis covers it.
      let grow = scale.abs().max(1.0);
      if *twist != 0.0 {
        let r = [min[0].abs(), max[0].abs(), min[1].abs(), max[1].abs()]
          .into_iter()
          .fold(0.0f32, f32::max)
          * grow;
        Some(([-r, -r, z0], [r, r, z1]))
      } else {
        Some((
          [min[0] * grow, min[1] * grow, z0],
          [max[0] * grow, max[1] * grow, z1],
        ))
      }
    }
    ScadNode::RotateExtrude { child, .. } => {
      // The profile's x extent becomes the radius, its y extent the height;
      // bound the full revolution regardless of the angle.
      let (min, max) = node_bbox(child)?;
      let r = min[0].abs().max(max[0].abs());
      Some(([-r, -r, min[1]], [r, r, max[1]]))
    }

    ScadNode::Translate { x, y, z, child } => {
      Some(transform_bbox(&mat4_translate(*x, *y, *z), node_bbox(child)?))
    }
    ScadNode::Rotate { x, y, z, child } => {
      // OpenSCAD rotation order: Z then Y then X.
      let m = mat4_mul(&mat4_rotate_z(*z), &mat4_rotate_y(*y));
      let m = mat4_mul(&m, &mat4_rotate_x(*x));
      Some(transform_bbox(&m, node_bbox(child)?))
    }
    ScadNode::Scale { x, y, z, child } => {
      Some(transform_bbox(&mat4_scale(*x, *y, *z), node_bbox(child)?))
    }
    ScadNode::Mirror { x, y, z, child } => {
      Some(transform_bbox(&mat4_mirror(*x, *y, *z), node_bbox(child)?))
    }
    ScadNode::Multmatrix { matrix, child } => Some(transform_bbox(
      &row_to_col_major(matrix),
      node_bbox(child)?,
    )),

    ScadNode::Color { child, .. }
    | ScadNode::Material { child, .. }
    | ScadNode::Render { child, .. }
    | ScadNode::Modifier { child, .. } => node_bbox(child),
    ScadNode::Offset {
      delta, r, child, ..
    } => {
      let (min, max) = node_bbox(child)?;
      let grow = delta.unwrap_or(0.0).abs().max(r.unwrap_or(0.0).abs());
      Some((
        [min[0] - grow, min[1] - grow, min[2]],
        [max[0] + grow, max[1] + grow, max[2]],
      ))
    }
    ScadNode::Projection { child, .. } => {
      let (min, max) = node_bbox(child)?;
      Some(([min[0], min[1], 0.0], [max[0], max[1], 0.0]))
    }

    // The hull of one subtree fills its box but never leaves it.
    ScadNode::Hull(child) => node_bbox(child),
    ScadNode::Union(children) => {
      let mut all: Option<Aabb> = None;
      for child in children {
        let b = node_bbox(child)?;
        all = Some(match all {
          None => b,
          Some(acc) => bbox_union(acc, b),
        });
      }
      all
    }
    // A Minkowski sum's box is the sum of the operand boxes.
    ScadNode::Minkowski(children) => {
      let mut sum: Option<Aabb> = None;
      for child in children {
        let b = node_bbox(child)?;
        sum = Some(match sum {
          None => b,
          Some((min, max)) => (
            [min[0] + b.0[0], min[1] + b.0[1], min[2] + b.0[2]],
            [max[0] + b.1[0], max[1] + b.1[1], max[2] + b.1[2]],
          ),
        });
      }
      sum
    }
    // A difference is contained in its base; unknown later operands
    // cannot grow it.
    ScadNode::Difference(children) => {
      node_bbox(children.iter().find(|c| !c.is_csg_dropped())?)
    }
    // An intersection is contained in every operand, so any known box
    // bounds it even when the others are unknown.
    ScadNode::Intersection(children) => {
      let mut overlap: Option<Aabb> = None;
      for child in children.iter().filter(|c| !c.is_csg_dropped()) {
        if let Some(b) = node_bbox(child) {
          overlap = Some(match overlap {
            None => b,
            Some(acc) => bbox_intersection(acc, b),
          });
        }
      }
      overlap
    }

    ScadNode::BoslCall {
      native: Some(native),
      ..
    } => node_bbox(native),

    // Imports, text, raw SCAD, BOSL previews: extent unknown.
    _ => None,
  }
}

/// True when a boolean operand dwarfs `reference` — extends beyond it by
/// more than the reference's own diagonal on some axis.
///
/// Such an operand — the giant sphere carving a shallow recess, the huge
/// cube cutting a model in half — breaks the image-space CSG pass: OpenCSG
/// needs both the front and back faces of every primitive inside the view
/// frustum, but the camera orbits at a distance set by the *result's* size,
/// so it routinely ends up inside the oversized operand. Its front faces
/// then fall behind the near plane, the stencil parity breaks, and the
/// subtraction quietly drops out — the carved-away stock pops back into
/// view at those angles. An unknown extent on either side keeps the
/// current behavior.
fn dwarfs(operand: &ScadNode, reference: Aabb) -> bool {
  let Some((omin, omax)) = node_bbox(operand) else {
    return false;
  };
  let (rmin, rmax) = reference;
  let diagonal = (0..3)
    .map(|axis| (rmax[axis] - rmin[axis]).powi(2))
    .sum::<f32>()
    .sqrt();
  (0..3).any(|axis| {
    omin[axis] < rmin[axis] - diagonal || omax[axis] > rmax[axis] + diagonal
  })
}

/// Returns true if `node`, sitting at `op` position, can be folded into the
/// single OpenCSG product `I1 ∩ … ∩ In − S1 − … − Sm` that its enclosing
/// boolean is flattened into.
///
/// Only some tree shapes fit. A union is a *sum* of products and a subtracted
/// difference expands to two products (`X − (A − B) = (X − A) ∪ (X ∩ B)`), so
/// neither can be appended to the enclosing product's operand list.
/// Non-tessellatable primitives (hull, Minkowski, extrusions) don't fit either
/// — they have no leaf tessellation to hand OpenCSG. Whatever doesn't fit is
/// computed by Manifold instead and rendered as a plain mesh.
fn fits_in_product(node: &ScadNode, op: c_int) -> bool {
  // A 2D shape has no OpenCSG form at all: its booleans combine areas, which
  // is Manifold's job, and the result draws as one flat mesh.
  if node_dimension(node) == Dimension::Two {
    return false;
  }

  match node {
    // Not tessellatable: needs Manifold materialization regardless of shape.
    ScadNode::Minkowski(_)
    | ScadNode::Hull(_)
    | ScadNode::LinearExtrude { .. }
    | ScadNode::RotateExtrude { .. } => false,

    // Transforms, colors, and materials are folded into the leaves they wrap.
    ScadNode::Translate { child, .. }
    | ScadNode::Rotate { child, .. }
    | ScadNode::Scale { child, .. }
    | ScadNode::Mirror { child, .. }
    | ScadNode::Multmatrix { child, .. }
    | ScadNode::Resize { child, .. }
    | ScadNode::Color { child, .. }
    | ScadNode::Material { child, .. } => fits_in_product(child, op),

    // A declared depth complexity above 1 marks a deeply concave shape
    // (a thread, an imported mesh). OpenCSG's layered Goldfeather pass
    // garbles those on the GL stacks the studio runs on — surface layers
    // drop out in facet-aligned stripes — so the product is computed by
    // Manifold instead. This also matches OpenSCAD, where `render()`
    // materializes its subtree at preview time.
    ScadNode::Render { convexity, child } => {
      *convexity <= 1 && fits_in_product(child, op)
    }
    ScadNode::Import { convexity, .. } => *convexity <= 1,

    // A heightmap's depth complexity is the number of ridges a grazing ray
    // crosses — unbounded, and not knowable from the declared convexity —
    // so it is always in the garbled class described above.
    ScadNode::Surface { .. } => false,

    // A native BOSL shape previews as its expansion, so whether it fits
    // is the expansion's call — a threaded rod hides a Render marker.
    ScadNode::BoslCall {
      native: Some(native),
      ..
    } => fits_in_product(native, op),

    ScadNode::Modifier { kind, child } => match kind {
      // Dropped from the CSG entirely, so they never widen the product.
      ModifierKind::Skip | ModifierKind::Transparent => true,
      ModifierKind::Debug => fits_in_product(child, op),
      // `!` replaces the whole scene; let Manifold materialize the boolean
      // so `collect_modifier_effects` can capture the subtree.
      ModifierKind::Only => false,
    },

    // `X − (A ∪ B)` = `X − A − B`, so a union collapses into the product
    // when every operand is subtracted — but not in an intersected position,
    // where it would turn into `A ∩ B`.
    ScadNode::Union(children) => {
      op == SUBTRACTION
        && children
          .iter()
          .all(|child| fits_in_product(child, SUBTRACTION))
    }

    // `X ∩ (A − B)` = `X ∩ A − B` keeps one product; subtracting the same
    // difference would not.
    ScadNode::Difference(children) => {
      let base = children.iter().position(|c| !c.is_csg_dropped());
      // A cutter dwarfing the base (see `dwarfs`) is measured against the
      // base's box — the result can be no larger.
      let base_bbox = base.and_then(|i| node_bbox(&children[i]));
      op == INTERSECTION
        && children.iter().enumerate().all(|(i, child)| {
          let child_op = if Some(i) == base {
            INTERSECTION
          } else {
            SUBTRACTION
          };
          fits_in_product(child, child_op)
            && !(child_op == SUBTRACTION
              && base_bbox.is_some_and(|b| dwarfs(child, b)))
        })
    }

    // Same reasoning: intersections nest into an intersected position only.
    // The dwarf check compares each operand against the overlap of all the
    // boxes — a huge half-space cube cutting a model in two is the common
    // case it catches.
    ScadNode::Intersection(children) => {
      let overlap = node_bbox(node);
      op == INTERSECTION
        && children.iter().all(|child| {
          fits_in_product(child, INTERSECTION)
            && !overlap.is_some_and(|b| dwarfs(child, b))
        })
    }

    // Everything else is a leaf primitive (or renders nothing at all).
    _ => true,
  }
}

fn flatten_inner(
  node: &ScadNode,
  ctx: &Ctx,
  op: c_int,
  sink: &mut ModifierSink,
) -> Vec<CsgGroup> {
  match node {
    // --- CSG booleans ---
    ScadNode::Union(children) => {
      // An area union is resolved by Manifold, not by drawing the operands
      // over each other — coplanar shapes would fight for the same depth.
      if node_dimension(node) == Dimension::Two {
        collect_modifier_effects(node, ctx, sink);
        return manifold_preview(node, ctx, op, 1);
      }
      // Each child of a union becomes its own group (separate OpenCSG render call).
      // Propagate `op` so that when a union appears inside a Difference (as a
      // subtracted operand), its leaves inherit the SUBTRACTION operation.
      let mut groups = Vec::new();
      for child in children {
        groups.extend(flatten_inner(child, ctx, op, sink));
      }
      groups
    }
    ScadNode::Difference(children) if !children.is_empty() => {
      // Shapes that don't fit a single OpenCSG product (nested unions and
      // differences, hulls, extrusions, …) are computed by Manifold as a
      // whole, which also avoids OpenCSG depth-buffer artifacts.
      if !fits_in_product(node, op) {
        collect_modifier_effects(node, ctx, sink);
        return manifold_preview(node, ctx, op, 1);
      }
      // First remaining child = Intersection, rest = Subtraction, all in one
      // group. `*`/`%` children are removed from the boolean entirely, so
      // they never become the base (OpenSCAD semantics).
      let mut leaves = Vec::new();
      let mut base_found = false;
      for child in children {
        let child_op = if !base_found && !child.is_csg_dropped() {
          base_found = true;
          INTERSECTION
        } else {
          SUBTRACTION
        };
        let child_groups = flatten_inner(child, ctx, child_op, sink);
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
      // Same fallback as for differences.
      if !fits_in_product(node, op) {
        collect_modifier_effects(node, ctx, sink);
        return manifold_preview(node, ctx, op, 1);
      }
      // All children are Intersection, in one group.
      let mut leaves = Vec::new();
      for child in children {
        let child_groups = flatten_inner(child, ctx, INTERSECTION, sink);
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
        ..*ctx
      };
      flatten_inner(child, &child_ctx, op, sink)
    }
    ScadNode::Rotate { x, y, z, child } => {
      // OpenSCAD rotation order: Z then Y then X
      let m = mat4_mul(&ctx.transform, &mat4_rotate_z(*z));
      let m = mat4_mul(&m, &mat4_rotate_y(*y));
      let m = mat4_mul(&m, &mat4_rotate_x(*x));
      let child_ctx = Ctx {
        transform: m,
        ..*ctx
      };
      flatten_inner(child, &child_ctx, op, sink)
    }
    ScadNode::Scale { x, y, z, child } => {
      let m = mat4_mul(&ctx.transform, &mat4_scale(*x, *y, *z));
      let child_ctx = Ctx {
        transform: m,
        ..*ctx
      };
      flatten_inner(child, &child_ctx, op, sink)
    }
    ScadNode::Mirror { x, y, z, child } => {
      let m = mat4_mul(&ctx.transform, &mat4_mirror(*x, *y, *z));
      let child_ctx = Ctx {
        transform: m,
        ..*ctx
      };
      flatten_inner(child, &child_ctx, op, sink)
    }
    ScadNode::Multmatrix { matrix, child } => {
      // ScadNode stores row-major; OpenGL wants column-major. Transpose.
      let col_major = row_to_col_major(matrix);
      let m = mat4_mul(&ctx.transform, &col_major);
      let child_ctx = Ctx {
        transform: m,
        ..*ctx
      };
      flatten_inner(child, &child_ctx, op, sink)
    }
    ScadNode::Resize { child, .. } => {
      // Resize is hard to decompose — just pass through with current transform.
      flatten_inner(child, ctx, op, sink)
    }
    ScadNode::Color { r, g, b, child, .. } => {
      let child_ctx = Ctx {
        color: Some([*r, *g, *b]),
        ..*ctx
      };
      flatten_inner(child, &child_ctx, op, sink)
    }
    ScadNode::Material { spec, child } => {
      let child_ctx = Ctx {
        material: *spec,
        ..*ctx
      };
      flatten_inner(child, &child_ctx, op, sink)
    }
    // `render(convexity = n)`: carry the promised depth complexity down to
    // the leaves, keeping the largest bound seen along the path.
    ScadNode::Render { convexity, child } => {
      let child_ctx = Ctx {
        convexity: ctx.convexity.max(*convexity),
        ..*ctx
      };
      flatten_inner(child, &child_ctx, op, sink)
    }

    // --- OpenSCAD modifier characters ---
    ScadNode::Modifier { kind, child } => match kind {
      // `*`: subtree is not rendered at all.
      ModifierKind::Skip => vec![],
      // `!`: capture the subtree (with ancestor transforms/color applied)
      // as the new scene root; everything else is discarded at the end of
      // flatten_geometries. First `!` wins, innermost on nesting.
      ModifierKind::Only => {
        if sink.only.is_none() {
          let mut sub = ModifierSink::default();
          let groups = flatten_inner(child, ctx, INTERSECTION, &mut sub);
          sink.only = Some(match sub.only {
            Some(inner) => inner,
            None => CsgScene {
              groups,
              overlays: sub.overlays,
            },
          });
        }
        vec![]
      }
      // `#`: participates in CSG normally, plus a translucent highlight
      // of the subtree's own shape (visible even where it is subtracted).
      ModifierKind::Debug => {
        if let Some(overlay) = overlay_mesh(child, ctx, HIGHLIGHT_COLOR) {
          sink.overlays.push(overlay);
        }
        flatten_inner(child, ctx, op, sink)
      }
      // `%`: removed from CSG, drawn as a translucent background object.
      ModifierKind::Transparent => {
        if let Some(overlay) = overlay_mesh(child, ctx, BACKGROUND_COLOR) {
          sink.overlays.push(overlay);
        }
        vec![]
      }
    },

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

    // --- 2D shapes: tessellated flat by Manifold ---
    node if node_dimension(node) == Dimension::Two => {
      manifold_preview(node, ctx, op, 1)
    }

    // --- Extrusions / Hull / Minkowski: not directly tessellatable ---
    // Materialize via Manifold and render the resulting mesh as a leaf.
    ScadNode::LinearExtrude { .. }
    | ScadNode::RotateExtrude { .. }
    | ScadNode::Hull(_)
    | ScadNode::Minkowski(_) => manifold_preview(node, ctx, op, 1),

    // Mesh files are read by the Manifold backend; the convexity the script
    // gave is passed on, since an imported part is often concave.
    ScadNode::Import { file, convexity }
      if luacad::mesh_import::is_mesh_file(file) =>
    {
      manifold_preview(node, ctx, op, (*convexity).max(1))
    }

    // A heightmap solid outside any boolean: materialized by Manifold like
    // an import. (Inside one, `fits_in_product` already routes the whole
    // product through Manifold.)
    ScadNode::Surface { convexity, .. } => {
      manifold_preview(node, ctx, op, (*convexity).max(1))
    }

    // --- BOSL2 shapes ---
    // A native shape is built from ordinary primitives, so it previews the
    // same way any other subtree does. Calls still waiting on a native
    // implementation fall back to their hand-written preview parameters.
    ScadNode::BoslCall {
      native: Some(native),
      ..
    } => flatten_inner(native, ctx, op, sink),

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
              ..*ctx
            };
            make_leaf_group(verts, &child_ctx, op, 1)
          }
          CylAxis::Y => {
            let m = mat4_mul(&ctx.transform, &mat4_rotate_x(-90.0));
            let child_ctx = Ctx {
              transform: m,
              ..*ctx
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
  let (vertices, transform) = bake_reflection(vertices, &ctx.transform);
  vec![CsgGroup {
    primitives: vec![CsgLeaf {
      vertices: cad_to_gl_vertices(vertices),
      transform,
      operation: op,
      // The call site's value is the shape's own floor; an enclosing
      // `render(convexity = n)` can only raise it.
      convexity: convexity.max(ctx.convexity),
      color: ctx.resolved_color(),
      material: ctx.material,
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

/// Materialize a modifier subtree via Manifold into a single translucent
/// overlay mesh. Returns None when the subtree yields no geometry (e.g.
/// BOSL shapes, which Manifold materialization doesn't cover yet).
fn overlay_mesh(
  node: &ScadNode,
  ctx: &Ctx,
  color: [f32; 4],
) -> Option<OverlayMesh> {
  let mesh = materialize_scad_display_mesh(node);
  if mesh.triangles.is_empty() {
    return None;
  }
  let mut verts = Vec::with_capacity(mesh.triangles.len() * 3);
  for tri in &mesh.triangles {
    verts.push(mesh.vertices[tri[0] as usize]);
    verts.push(mesh.vertices[tri[1] as usize]);
    verts.push(mesh.vertices[tri[2] as usize]);
  }
  let (verts, transform) = bake_reflection(verts, &ctx.transform);
  Some(OverlayMesh {
    vertices: cad_to_gl_vertices(verts),
    transform,
    color,
  })
}

/// Emit the modifier side effects (`#`/`%` overlays, `!` capture) for a
/// subtree that is materialized as a whole via Manifold instead of being
/// recursed into by `flatten_inner`.
fn collect_modifier_effects(
  node: &ScadNode,
  ctx: &Ctx,
  sink: &mut ModifierSink,
) {
  match node {
    ScadNode::Union(children)
    | ScadNode::Difference(children)
    | ScadNode::Intersection(children)
    | ScadNode::Minkowski(children) => {
      for child in children {
        collect_modifier_effects(child, ctx, sink);
      }
    }
    ScadNode::Hull(child) => collect_modifier_effects(child, ctx, sink),
    ScadNode::Translate { x, y, z, child } => {
      let m = mat4_mul(&ctx.transform, &mat4_translate(*x, *y, *z));
      let child_ctx = Ctx {
        transform: m,
        ..*ctx
      };
      collect_modifier_effects(child, &child_ctx, sink);
    }
    ScadNode::Rotate { x, y, z, child } => {
      let m = mat4_mul(&ctx.transform, &mat4_rotate_z(*z));
      let m = mat4_mul(&m, &mat4_rotate_y(*y));
      let m = mat4_mul(&m, &mat4_rotate_x(*x));
      let child_ctx = Ctx {
        transform: m,
        ..*ctx
      };
      collect_modifier_effects(child, &child_ctx, sink);
    }
    ScadNode::Scale { x, y, z, child } => {
      let m = mat4_mul(&ctx.transform, &mat4_scale(*x, *y, *z));
      let child_ctx = Ctx {
        transform: m,
        ..*ctx
      };
      collect_modifier_effects(child, &child_ctx, sink);
    }
    ScadNode::Mirror { x, y, z, child } => {
      let m = mat4_mul(&ctx.transform, &mat4_mirror(*x, *y, *z));
      let child_ctx = Ctx {
        transform: m,
        ..*ctx
      };
      collect_modifier_effects(child, &child_ctx, sink);
    }
    ScadNode::Multmatrix { matrix, child } => {
      let m = mat4_mul(&ctx.transform, &row_to_col_major(matrix));
      let child_ctx = Ctx {
        transform: m,
        ..*ctx
      };
      collect_modifier_effects(child, &child_ctx, sink);
    }
    ScadNode::Resize { child, .. }
    | ScadNode::Color { child, .. }
    | ScadNode::Material { child, .. }
    | ScadNode::Render { child, .. } => {
      collect_modifier_effects(child, ctx, sink);
    }
    ScadNode::Modifier { kind, child } => match kind {
      ModifierKind::Skip => {}
      ModifierKind::Only => {
        if sink.only.is_none() {
          let mut sub = ModifierSink::default();
          let groups = flatten_inner(child, ctx, INTERSECTION, &mut sub);
          sink.only = Some(match sub.only {
            Some(inner) => inner,
            None => CsgScene {
              groups,
              overlays: sub.overlays,
            },
          });
        }
      }
      ModifierKind::Debug => {
        if let Some(overlay) = overlay_mesh(child, ctx, HIGHLIGHT_COLOR) {
          sink.overlays.push(overlay);
        }
        collect_modifier_effects(child, ctx, sink);
      }
      ModifierKind::Transparent => {
        if let Some(overlay) = overlay_mesh(child, ctx, BACKGROUND_COLOR) {
          sink.overlays.push(overlay);
        }
      }
    },
    _ => {}
  }
}

/// Build a ScadNode tree from primitives, materialize it via Manifold, and
/// return CsgGroups ready for OpenCSG rendering.
fn manifold_preview(
  node: &ScadNode,
  ctx: &Ctx,
  op: c_int,
  convexity: u32,
) -> Vec<CsgGroup> {
  // Dimension-aware, so a 2D shape shows up flat instead of not at all.
  let mesh = materialize_scad_display_mesh(node);
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

#[cfg(test)]
mod reflection_tests {
  use super::*;

  fn signed_volume(vertices: &[[f32; 3]]) -> f32 {
    vertices
      .as_chunks::<3>()
      .0
      .iter()
      .map(|t| {
        let (a, b, c) = (t[0], t[1], t[2]);
        (a[0] * (b[1] * c[2] - b[2] * c[1])
          - a[1] * (b[0] * c[2] - b[2] * c[0])
          + a[2] * (b[0] * c[1] - b[1] * c[0]))
          / 6.0
      })
      .sum()
  }

  /// A lone mirror must not leave the leaf inside out: the reflection
  /// is baked into the vertices with every triangle reversed, so the
  /// mesh keeps a positive orientation and back-face culling keeps the
  /// part visible in the shaded preview.
  #[test]
  fn lone_mirror_keeps_leaf_right_side_out() {
    let geoms =
      luacad::lua_engine::execute_lua("render(cube(10):mirror(1, 0, 0))")
        .unwrap();
    let scene = flatten_geometries(&geoms);
    let leaf = &scene.groups[0].primitives[0];
    assert_eq!(leaf.transform, IDENTITY, "the reflection must be baked in");
    assert!(
      signed_volume(&leaf.vertices) > 0.0,
      "mirrored leaf must keep a positive orientation"
    );
  }

  /// An even mirror count is a plain rotation: the transform reflects
  /// nothing, stays on the leaf, and the vertices are left alone.
  #[test]
  fn double_mirror_stays_on_the_leaf() {
    let geoms = luacad::lua_engine::execute_lua(
      "render(cube(10):mirror(1, 0, 0):mirror(0, 1, 0))",
    )
    .unwrap();
    let scene = flatten_geometries(&geoms);
    let leaf = &scene.groups[0].primitives[0];
    assert!(mat4_det3(&leaf.transform) > 0.0);
    assert!(signed_volume(&leaf.vertices) > 0.0);
  }
}

#[cfg(test)]
mod modifier_tests {
  use super::*;

  fn flatten_lua(code: &str) -> CsgScene {
    let geoms = luacad::lua_engine::execute_lua(code).unwrap();
    flatten_geometries(&geoms)
  }

  #[test]
  fn skip_removes_subtree_from_preview() {
    let scene = flatten_lua("render(s(cube(10)))");
    assert!(scene.groups.is_empty(), "skipped object must not render");
    assert!(scene.overlays.is_empty());
  }

  #[test]
  fn only_replaces_whole_scene() {
    let scene = flatten_lua(
      r#"
      render(cube({ 20, 20, 10 }))
      render(o(sphere({ r = 5 }):translate(10, 30, 5)))
      render(cube({ 8, 8, 8 }):translate(-15, 0, 0))
      "#,
    );
    assert_eq!(scene.groups.len(), 1, "only the `!` subtree must remain");
    let leaf = &scene.groups[0].primitives[0];
    assert_eq!(leaf.operation, INTERSECTION);
    assert_eq!(
      [leaf.transform[12], leaf.transform[13], leaf.transform[14]],
      [10.0, 30.0, 5.0],
      "ancestor transform must be preserved"
    );
  }

  #[test]
  fn debug_keeps_csg_and_adds_highlight_overlay() {
    let scene = flatten_lua(
      "render(cube({ 20, 20, 10 }) - d(cylinder({ h = 12, r = 4 })))",
    );
    assert_eq!(scene.groups.len(), 1);
    let ops: Vec<_> = scene.groups[0]
      .primitives
      .iter()
      .map(|p| p.operation)
      .collect();
    assert_eq!(
      ops,
      vec![INTERSECTION, SUBTRACTION],
      "`#` must still participate in the difference"
    );
    assert_eq!(scene.overlays.len(), 1, "`#` must add a highlight overlay");
    assert_eq!(scene.overlays[0].color, HIGHLIGHT_COLOR);
  }

  #[test]
  fn transparent_is_dropped_from_difference_base() {
    // `%` first child: the sphere becomes the base instead (OpenSCAD
    // background semantics), and the cube is drawn as a gray overlay.
    let scene = flatten_lua("render(t(cube(10)) - sphere({ r = 5 }))");
    let leaves: Vec<_> =
      scene.groups.iter().flat_map(|g| &g.primitives).collect();
    assert_eq!(leaves.len(), 1, "only the sphere must remain in the CSG");
    assert_eq!(leaves[0].operation, INTERSECTION);
    assert_eq!(scene.overlays.len(), 1, "`%` must add a background overlay");
    assert_eq!(scene.overlays[0].color, BACKGROUND_COLOR);
  }
}

#[cfg(test)]
mod product_tests {
  use super::*;

  fn flatten_lua(code: &str) -> CsgScene {
    let geoms = luacad::lua_engine::execute_lua(code).unwrap();
    flatten_geometries(&geoms)
  }

  /// A single Manifold-materialized mesh: one group, one intersected leaf.
  fn assert_single_mesh(scene: &CsgScene, what: &str) {
    assert_eq!(scene.groups.len(), 1, "{what}: expected one group");
    let prims = &scene.groups[0].primitives;
    assert_eq!(prims.len(), 1, "{what}: expected one materialized leaf");
    assert_eq!(prims[0].operation, INTERSECTION);
    assert!(
      !prims[0].vertices.is_empty(),
      "{what}: leaf must have geometry"
    );
  }

  #[test]
  fn render_node_materializes_the_product() {
    // `render(convexity = n)` with n > 1 declares a shape too deep for
    // OpenCSG's layered pass; the whole product drops to Manifold, like
    // OpenSCAD materializing a `render()` subtree at preview time.
    let scene = flatten_lua(
      "render(cube({ 20, 20, 20 })
        - cylinder({ r = 4, h = 30 }):render_node(7))",
    );
    assert_single_mesh(&scene, "X - render(convexity = 7)");
  }

  #[test]
  fn subtracted_thread_is_materialized() {
    // A threaded rod is deeply concave: a ray along the axis crosses one
    // crest per pitch. Previewed as a subtracted OpenCSG primitive, its
    // deeper surface layers came out garbled, leaving brick-shaped holes
    // across the bore wall — so the thread's own convexity declaration
    // must drop the whole product to Manifold.
    let scene = flatten_lua(
      r#"
      render(
        cylinder({ r = 9, h = 29 }):translate(0, 0, -14.5)
        - bosl.trapezoidal_threaded_rod({
          d = 10, l = 31, pitch = 3, thread_angle = 90,
          thread_depth = 1.2, internal = true, slop = 0.4,
        })
      )
      "#,
    );
    assert_single_mesh(&scene, "X - threaded_rod");
  }

  #[test]
  fn subtracted_difference_is_materialized() {
    // `X - (A - B)` is not one OpenCSG product: flattening it in place would
    // render `X ∩ A - B` and drop the island `B` leaves inside the cavity.
    let scene = flatten_lua(
      r#"
      local outer = cube({ 40, 40, 40 })
      local cavity = cube({ 30, 30, 30 }):translate(5, 5, 5)
      local island = cube({ 10, 10, 10 }):translate(15, 15, 15)
      render(outer - (cavity - island))
      "#,
    );
    assert_single_mesh(&scene, "X - (A - B)");
  }

  #[test]
  fn subtracted_intersection_is_materialized() {
    let scene = flatten_lua(
      r#"
      local outer = cube({ 40, 40, 40 })
      local a = cube({ 30, 30, 30 }):translate(5, 5, 5)
      local b = sphere({ r = 18 }):translate(20, 20, 20)
      render(outer - (a * b))
      "#,
    );
    assert_single_mesh(&scene, "X - (A ∩ B)");
  }

  #[test]
  fn union_base_of_difference_is_materialized() {
    // `(A ∪ B) - S` is a sum of two products, not one: flattening it in place
    // would render `A ∩ B - S`.
    let scene = flatten_lua(
      r#"
      local a = cube({ 20, 20, 20 })
      local b = cube({ 20, 20, 20 }):translate(30, 0, 0)
      render((a + b) - cylinder({ r = 4, h = 40 }):translate(10, 10, -10))
      "#,
    );
    assert_single_mesh(&scene, "(A ∪ B) - S");
  }

  #[test]
  fn union_inside_intersection_is_materialized() {
    let scene = flatten_lua(
      r#"
      local a = cube({ 20, 20, 20 })
      local b = cube({ 20, 20, 20 }):translate(10, 0, 0)
      render(cube({ 40, 40, 40 }) * (a + b))
      "#,
    );
    assert_single_mesh(&scene, "X ∩ (A ∪ B)");
  }

  #[test]
  fn subtracted_union_still_uses_opencsg() {
    // `X - (A ∪ B)` = `X - A - B` does fit one product, so it must keep the
    // cheap OpenCSG path instead of falling back to Manifold.
    let scene = flatten_lua(
      r#"
      local holes = cylinder({ r = 3, h = 30 }):translate(5, 5, -5)
        + cylinder({ r = 3, h = 30 }):translate(15, 15, -5)
      render(cube({ 20, 20, 20 }) - holes)
      "#,
    );
    assert_eq!(scene.groups.len(), 1);
    let ops: Vec<_> = scene.groups[0]
      .primitives
      .iter()
      .map(|p| p.operation)
      .collect();
    assert_eq!(ops, vec![INTERSECTION, SUBTRACTION, SUBTRACTION]);
  }

  /// Write an OFF tetrahedron somewhere `import()` can read it back.
  fn temp_tetrahedron() -> String {
    use std::io::Write;
    use std::sync::atomic::{AtomicUsize, Ordering};
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    let path = std::env::temp_dir().join(format!(
      "luacad_studio_import_{}_{}.off",
      std::process::id(),
      COUNTER.fetch_add(1, Ordering::Relaxed),
    ));
    let mut file = std::fs::File::create(&path).unwrap();
    file
      .write_all(
        b"OFF\n4 4 0\n0 0 0\n10 0 0\n0 10 0\n0 0 10\n\
          3 0 2 1\n3 0 1 3\n3 0 3 2\n3 1 2 3\n",
      )
      .unwrap();
    path.to_str().unwrap().to_string()
  }

  #[test]
  fn an_imported_mesh_previews_as_a_leaf() {
    let path = temp_tetrahedron();
    let scene = flatten_lua(&format!("render(import(\"{path}\"))"));
    assert_single_mesh(&scene, "import()");
    let _ = std::fs::remove_file(&path);
  }

  #[test]
  fn an_imported_mesh_can_be_subtracted_in_one_product() {
    // The imported leaf carries its own tessellation, so OpenCSG can take it
    // as a product operand instead of forcing a Manifold materialization.
    let path = temp_tetrahedron();
    let scene = flatten_lua(&format!(
      "render(cube({{ 20, 20, 20 }}) - import(\"{path}\"))"
    ));
    assert_eq!(scene.groups.len(), 1);
    let ops: Vec<_> = scene.groups[0]
      .primitives
      .iter()
      .map(|p| p.operation)
      .collect();
    assert_eq!(ops, vec![INTERSECTION, SUBTRACTION]);
    let _ = std::fs::remove_file(&path);
  }

  #[test]
  fn extruded_text_previews_as_a_leaf() {
    let scene = flatten_lua(
      r#"render(text("H", { size = 10, font = "sans-serif" }):linear_extrude(2))"#,
    );
    // A machine with no fonts installed has nothing to outline.
    if scene.groups.is_empty() {
      return;
    }
    assert_single_mesh(&scene, "extruded text()");
  }

  #[test]
  fn intersected_difference_still_uses_opencsg() {
    // `X ∩ (A - B)` = `X ∩ A - B` fits one product.
    let scene = flatten_lua(
      r#"
      local ring = cube({ 20, 20, 20 }) - cylinder({ r = 4, h = 30 }):translate(10, 10, -5)
      render(sphere({ r = 14 }) * ring)
      "#,
    );
    assert_eq!(scene.groups.len(), 1);
    let ops: Vec<_> = scene.groups[0]
      .primitives
      .iter()
      .map(|p| p.operation)
      .collect();
    assert_eq!(ops, vec![INTERSECTION, INTERSECTION, SUBTRACTION]);
  }
}

#[cfg(test)]
mod tests {
  use super::*;
  use luacad::scad_export::ScadNode;

  fn geometry(scad: ScadNode) -> CsgGeometry {
    CsgGeometry {
      mesh: None,
      color: None,
      material: None,
      scad: Some(scad),
      name: None,
    }
  }

  /// A 2D shape is output in its own right, so the viewport has to receive
  /// something to draw for it rather than an empty group.
  #[test]
  fn an_outline_reaches_the_viewport() {
    let scene = flatten_geometries(&[geometry(ScadNode::Difference(vec![
      ScadNode::Square {
        w: 20.0,
        h: 10.0,
        center: true,
      },
      ScadNode::Circle {
        r: 3.0,
        segments: 32,
      },
    ]))]);

    let vertices: usize = scene
      .groups
      .iter()
      .flat_map(|group| &group.primitives)
      .map(|leaf| leaf.vertices.len())
      .sum();
    assert!(vertices > 0, "a 2D shape produced nothing to draw");
  }

  /// A `surface()` heightmap inside a boolean must reach the normal preview.
  /// It used to fall through the leaf walker's catch-all while still counting
  /// as part of the OpenCSG product, so the emblem it carved simply vanished
  /// from the shaded view (and only appeared in transparent mode).
  #[test]
  fn a_heightmap_reaches_the_viewport() {
    let dir = std::env::temp_dir().join("luacad_studio_test_surface");
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("heights.dat");
    std::fs::write(&file, "0 2\n2 0\n").unwrap();

    let surface = ScadNode::Surface {
      file: file.to_string_lossy().into_owned(),
      center: false,
      convexity: 1,
      invert: false,
    };
    let cube = ScadNode::Cube {
      w: 1.0,
      d: 1.0,
      h: 1.0,
      center: false,
    };
    for scad in [surface.clone(), ScadNode::Intersection(vec![surface, cube])] {
      let scene = flatten_geometries(&[geometry(scad)]);
      let vertices: usize = scene
        .groups
        .iter()
        .flat_map(|group| &group.primitives)
        .map(|leaf| leaf.vertices.len())
        .sum();
      assert!(vertices > 0, "a heightmap produced nothing to draw");
    }
  }

  /// A cutter that dwarfs its base — the medal's 500-radius sphere carving a
  /// shallow recess out of a 150-radius disc — must not become an OpenCSG
  /// leaf: the camera orbits at the result's scale, ends up inside the
  /// sphere, and the subtraction drops out at those angles. The product is
  /// materialized by Manifold instead, which shows as a single primitive.
  #[test]
  fn a_giant_cutter_is_materialized_not_peeled() {
    let base = ScadNode::Translate {
      x: 0.0,
      y: 0.0,
      z: -99.0,
      child: Box::new(ScadNode::Cylinder {
        r1: 150.0,
        r2: 150.0,
        h: 100.0,
        segments: 64,
        center: false,
      }),
    };
    let cutter = ScadNode::Translate {
      x: 0.0,
      y: 0.0,
      z: -500.0,
      child: Box::new(ScadNode::Sphere {
        r: 500.0,
        segments: 64,
      }),
    };
    let scene =
      flatten_geometries(&[geometry(ScadNode::Difference(vec![base, cutter]))]);
    let leaves: usize =
      scene.groups.iter().map(|g| g.primitives.len()).sum();
    assert_eq!(leaves, 1, "expected one materialized mesh, not a product");
    assert!(scene.groups[0].primitives[0].vertices.len() > 0);
  }

  /// An ordinary subtraction — a bolt hole overshooting its plate a little —
  /// keeps the interactive per-primitive OpenCSG path.
  #[test]
  fn a_proportionate_cutter_still_forms_a_product() {
    let base = ScadNode::Cube {
      w: 20.0,
      d: 20.0,
      h: 10.0,
      center: true,
    };
    let cutter = ScadNode::Cylinder {
      r1: 3.0,
      r2: 3.0,
      h: 12.0,
      segments: 32,
      center: true,
    };
    let scene =
      flatten_geometries(&[geometry(ScadNode::Difference(vec![base, cutter]))]);
    let leaves: usize =
      scene.groups.iter().map(|g| g.primitives.len()).sum();
    assert_eq!(leaves, 2, "expected an OpenCSG product of base and cutter");
  }
}
