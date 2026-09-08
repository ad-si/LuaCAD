//! The flattened scene: what [`crate::flatten`] produces and [`crate::render`]
//! draws.
//!
//! These types carry no engine and no GPU types, so they are available in
//! every build of the crate — including the playground's viewer module, which
//! decodes them from [`crate::wire`] without an engine to flatten with.

#[cfg(feature = "flatten")]
use luacad::material::{MaterialKind, MaterialSpec};

/// How a primitive takes part in the CSG product it belongs to.
///
/// The product is the intersection of all [`Operation::Intersection`]
/// primitives minus all [`Operation::Subtraction`] ones — WebCSG's model, and
/// OpenCSG's before it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Operation {
  Intersection,
  Subtraction,
}

/// The surface parameters of one draw, with the material already resolved to
/// the Blinn-Phong terms the shader wants.
///
/// Resolving on the flattening side keeps [`luacad::material`] out of the
/// renderer, which is what lets the viewer module build without the engine.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Shading {
  /// Diffuse (and ambient) color, 0..1
  pub color: [f32; 3],
  /// Specular color in `rgb`, shininess in `a`
  pub specular: [f32; 4],
  /// Emitted color, 0..1, black for everything but an emissive material
  pub emission: [f32; 3],
}

impl Shading {
  /// An unlit color, drawn with no highlight — what a mesh without a material
  /// (an overlay, a csgrs fallback) gets.
  pub const fn plain(color: [f32; 3]) -> Self {
    Self {
      color,
      specular: [0.0, 0.0, 0.0, 1.0],
      emission: [0.0; 3],
    }
  }

  /// The terms of a surface of `color` made of `material`, through the
  /// material's Blinn-Phong approximation — the same mapping the software
  /// rasterizer uses, so the preview matches `luacad render`.
  #[cfg(feature = "flatten")]
  pub fn from_material(color: [f32; 3], material: &MaterialSpec) -> Self {
    let [r, g, b] = color;
    if material.kind == MaterialKind::Emissive {
      // Unlit: all radiance comes from the emission term. Overbright values
      // are normalized by the largest channel rather than clamped per
      // channel, which would wash saturated colors out to white.
      let s = material.strength;
      let max = (r.max(g).max(b) * s).max(1.0);
      let n = s / max;
      return Self {
        color: [0.0; 3],
        specular: [0.0, 0.0, 0.0, 1.0],
        emission: [r * n, g * n, b * n],
      };
    }
    let params = material.blinn_phong();
    let d = params.diffuse_scale;
    let s = params.specular_strength;
    let specular = if params.tinted_specular {
      [s * r, s * g, s * b]
    } else {
      [s, s, s]
    };
    Self {
      color: [r * d, g * d, b * d],
      // Fixed-function GL clamped the shininess to 128; the preview keeps
      // that look
      specular: [
        specular[0],
        specular[1],
        specular[2],
        params.shininess.min(128.0),
      ],
      emission: [0.0; 3],
    }
  }
}

impl Default for Shading {
  fn default() -> Self {
    Self::plain([1.0; 3])
  }
}

/// A single leaf primitive ready for CSG rendering.
#[derive(Debug, Clone)]
pub struct CsgLeaf {
  /// Triangle vertices (groups of 3 positions). GL coordinates (Y-up).
  pub vertices: Vec<[f32; 3]>,
  /// Accumulated model-to-world transform (column-major 4x4).
  pub transform: [f32; 16],
  /// Whether the leaf is intersected with or subtracted from its product.
  pub operation: Operation,
  /// Convexity (max front faces at a single point). 1 for convex shapes.
  pub convexity: u32,
  /// Color and material of the surface.
  pub shading: Shading,
}

/// A group of primitives that form a single CSG render call.
#[derive(Debug, Clone, Default)]
pub struct CsgGroup {
  pub primitives: Vec<CsgLeaf>,
}

/// A translucent mesh drawn over the CSG result in a blended pass:
/// `#` (debug highlight) and `%` (background) modifier geometry.
#[derive(Debug, Clone)]
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
#[derive(Debug, Clone)]
pub struct SolidMesh {
  /// Triangle vertices (groups of 3 positions). GL coordinates (Y-up), with
  /// every transform already applied.
  pub vertices: Vec<[f32; 3]>,
  /// Color and material of the surface.
  pub shading: Shading,
}

/// Everything needed to draw the preview: opaque CSG groups plus
/// translucent modifier overlays.
#[derive(Debug, Clone, Default)]
pub struct CsgScene {
  pub groups: Vec<CsgGroup>,
  pub overlays: Vec<OverlayMesh>,
}

impl CsgScene {
  /// Number of triangles the scene draws, the CSG primitives and the
  /// overlays together.
  pub fn triangle_count(&self) -> usize {
    let leaves: usize = self
      .groups
      .iter()
      .flat_map(|group| &group.primitives)
      .map(|leaf| leaf.vertices.len())
      .sum();
    let overlays: usize = self
      .overlays
      .iter()
      .map(|o| o.vertices.len())
      .sum::<usize>();
    (leaves + overlays) / 3
  }

  /// Radius of the smallest origin-centered sphere containing the scene's
  /// solid material, in GL coordinates. `None` for an empty scene.
  ///
  /// Only the intersected primitives are measured: a subtracted one is a
  /// cutter that may reach far past the model it carves, and framing the view
  /// on it would leave the model a speck in the middle.
  pub fn extent(&self) -> Option<f32> {
    let mut extent: f32 = 0.0;
    for leaf in self.groups.iter().flat_map(|group| &group.primitives) {
      if leaf.operation != Operation::Intersection {
        continue;
      }
      for vertex in &leaf.vertices {
        extent = extent
          .max(point_magnitude(transform_point(&leaf.transform, *vertex)));
      }
    }
    (extent >= 1e-6).then_some(extent)
  }
}

/// Radius of the smallest origin-centered sphere containing the materialized
/// scene, in GL coordinates.
///
/// Takes the solids rather than the geometries so that fitting the view and
/// the transparent pass share one materialization — the Manifold booleans
/// behind them are far too slow to run twice, let alone in the render loop.
pub fn solids_extent(solids: &[SolidMesh]) -> Option<f32> {
  let mut extent: f32 = 0.0;
  for solid in solids {
    for vertex in &solid.vertices {
      extent = extent.max(point_magnitude(*vertex));
    }
  }
  (extent >= 1e-6).then_some(extent)
}

/// Camera distance needed to fit a scene of the given extent in view.
pub fn fit_distance_for_extent(extent: f32, orthogonal: bool) -> f32 {
  let padding = 1.3;
  if orthogonal {
    extent * padding
  } else {
    // Half of the perspective field of view, see the studio's `build_camera`
    // and the playground viewer's camera.
    extent * padding / 22.5_f32.to_radians().tan()
  }
}

fn point_magnitude([x, y, z]: [f32; 3]) -> f32 {
  (x * x + y * y + z * z).sqrt()
}

/// Apply a column-major affine 4x4 matrix to a point.
fn transform_point(m: &[f32; 16], p: [f32; 3]) -> [f32; 3] {
  [
    m[0] * p[0] + m[4] * p[1] + m[8] * p[2] + m[12],
    m[1] * p[0] + m[5] * p[1] + m[9] * p[2] + m[13],
    m[2] * p[0] + m[6] * p[1] + m[10] * p[2] + m[14],
  ]
}

#[cfg(test)]
mod tests {
  use super::*;

  fn leaf(operation: Operation, vertices: Vec<[f32; 3]>) -> CsgLeaf {
    CsgLeaf {
      vertices,
      transform: [
        1.0, 0.0, 0.0, 0.0, //
        0.0, 1.0, 0.0, 0.0, //
        0.0, 0.0, 1.0, 0.0, //
        0.0, 0.0, 0.0, 1.0,
      ],
      operation,
      convexity: 1,
      shading: Shading::default(),
    }
  }

  /// Fitting the view on a hole's cutter — which routinely overshoots the
  /// model by a good margin — would zoom the model itself out of sight.
  #[test]
  fn the_extent_ignores_subtracted_primitives() {
    let scene = CsgScene {
      groups: vec![CsgGroup {
        primitives: vec![
          leaf(Operation::Intersection, vec![[0.0, 0.0, 3.0]]),
          leaf(Operation::Subtraction, vec![[0.0, 0.0, 300.0]]),
        ],
      }],
      overlays: vec![],
    };
    assert_eq!(scene.extent(), Some(3.0));
  }

  /// The leaf's transform is what places it in the world, so the extent has
  /// to measure the vertices through it.
  #[test]
  fn the_extent_measures_transformed_vertices() {
    let mut moved = leaf(Operation::Intersection, vec![[0.0, 0.0, 0.0]]);
    moved.transform[13] = 4.0;
    let scene = CsgScene {
      groups: vec![CsgGroup {
        primitives: vec![moved],
      }],
      overlays: vec![],
    };
    assert_eq!(scene.extent(), Some(4.0));
    assert_eq!(CsgScene::default().extent(), None, "nothing to fit on");
  }
}
