//! Vertices-and-faces meshes, the workhorse behind the native BOSL2 solids.
//!
//! Most BOSL2 shapes are a grid of points swept along an axis or around one —
//! a cylinder, a torus, a prismoid, a threaded rod — so they all come out of
//! [`Vnf::vertex_array`]. Shapes that are a profile revolved or extruded get
//! the thin wrappers below.
//!
//! Faces here are wound **counter-clockwise seen from outside**, so a face's
//! right-hand-rule normal points out of the solid. [`Vnf::to_node`] reverses
//! them on the way out, because OpenSCAD's `polyhedron()` — which
//! [`ScadNode::Polyhedron`] mirrors — wants the opposite order.

use crate::bosl::vecmath::{V2, V3, cross, norm, sub};
use crate::scad_export::ScadNode;

/// A mesh as a point list plus faces indexing into it.
#[derive(Clone, Debug, Default)]
pub struct Vnf {
  pub points: Vec<V3>,
  pub faces: Vec<Vec<usize>>,
}

/// Which ends of a swept surface are closed off.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Caps {
  pub start: bool,
  pub end: bool,
}

impl Caps {
  pub const BOTH: Caps = Caps {
    start: true,
    end: true,
  };
  pub const NONE: Caps = Caps {
    start: false,
    end: false,
  };
}

impl Vnf {
  pub fn new() -> Vnf {
    Vnf::default()
  }

  pub fn is_empty(&self) -> bool {
    self.faces.is_empty() || self.points.is_empty()
  }

  /// Append another mesh, shifting its face indices to match.
  pub fn join(&mut self, other: &Vnf) {
    let base = self.points.len();
    self.points.extend_from_slice(&other.points);
    self.faces.extend(
      other
        .faces
        .iter()
        .map(|f| f.iter().map(|i| i + base).collect()),
    );
  }

  pub fn joined(meshes: &[Vnf]) -> Vnf {
    let mut out = Vnf::new();
    for m in meshes {
      out.join(m);
    }
    out
  }

  /// Reverse every face, turning the solid inside out.
  pub fn reversed(&self) -> Vnf {
    Vnf {
      points: self.points.clone(),
      faces: self
        .faces
        .iter()
        .map(|f| f.iter().rev().copied().collect())
        .collect(),
    }
  }

  /// Move every vertex through `f`.
  pub fn map_points(&self, f: impl Fn(V3) -> V3) -> Vnf {
    Vnf {
      points: self.points.iter().map(|p| f(*p)).collect(),
      faces: self.faces.clone(),
    }
  }

  /// Build a surface from a grid of points.
  ///
  /// Each entry of `rows` is one row of the grid, all the same length.
  /// `col_wrap` joins the last column back to the first — what turns a sheet
  /// into a tube — and `row_wrap` does the same along the rows, closing a
  /// tube into a torus. `caps` closes off the first and last rows with flat
  /// faces, which only makes sense when the rows are closed loops.
  pub fn vertex_array(
    rows: &[Vec<V3>],
    caps: Caps,
    col_wrap: bool,
    row_wrap: bool,
  ) -> Vnf {
    let n_rows = rows.len();
    if n_rows < 2 {
      return Vnf::new();
    }
    let n_cols = rows[0].len();
    if n_cols < 2 || rows.iter().any(|r| r.len() != n_cols) {
      return Vnf::new();
    }

    let points: Vec<V3> = rows.iter().flatten().copied().collect();
    let idx = |r: usize, c: usize| (r % n_rows) * n_cols + (c % n_cols);

    let row_end = if row_wrap { n_rows } else { n_rows - 1 };
    let col_end = if col_wrap { n_cols } else { n_cols - 1 };

    let mut faces: Vec<Vec<usize>> = Vec::new();

    // A flat cap is the whole row as one polygon. The first row is reversed
    // because its outward direction is opposite the sweep.
    if caps.start && !row_wrap {
      faces.push((0..n_cols).rev().collect());
    }

    for r in 0..row_end {
      for c in 0..col_end {
        let i1 = idx(r, c);
        let i2 = idx(r + 1, c);
        let i3 = idx(r + 1, c + 1);
        let i4 = idx(r, c + 1);
        // Split each quad along the i1–i3 diagonal, BOSL2's default style.
        // Rows advance along the sweep and columns run counter-clockwise
        // around it, so this order is the one that faces outward.
        for tri in [[i1, i3, i2], [i1, i4, i3]] {
          if !is_degenerate(&points, &tri) {
            faces.push(tri.to_vec());
          }
        }
      }
    }

    if caps.end && !row_wrap {
      let base = (n_rows - 1) * n_cols;
      faces.push((0..n_cols).map(|c| base + c).collect());
    }

    Vnf { points, faces }
  }

  /// Revolve a profile around the Z axis.
  ///
  /// The profile is given in the XZ half-plane as `[radius, z]` pairs, wound
  /// so that increasing index runs counter-clockwise in that plane.
  ///
  /// `profile_closed` says whether the profile is a closed loop — a tube's
  /// rectangular cross-section — or an open path whose ends meet the axis,
  /// as a sphere's pole-to-pole arc does. A full turn wraps the surface
  /// closed; a partial one is capped at the two cut faces.
  pub fn rotate_sweep(
    profile: &[V2],
    angle: f64,
    segments: u32,
    profile_closed: bool,
  ) -> Vnf {
    let full_turn = (angle.abs() - 360.0).abs() < 1e-9;
    let n = segments.max(3) as usize;
    let steps = if full_turn { n } else { n + 1 };
    // The revolution runs counter-clockwise about Z, so walking the profile
    // backwards is what leaves the surface facing outward.
    let rows: Vec<Vec<V3>> = (0..steps)
      .map(|i| {
        let a = angle * i as f64 / n as f64;
        let (s, c) = a.to_radians().sin_cos();
        profile
          .iter()
          .rev()
          .map(|p| [p[0] * c, p[0] * s, p[1]])
          .collect()
      })
      .collect();
    // Rows run around the axis and columns along the profile. A partial
    // revolution leaves the first and last cross-section open, and those are
    // only fillable when the profile is a closed loop.
    let caps = if !full_turn && profile_closed {
      Caps::BOTH
    } else {
      Caps::NONE
    };
    Vnf::vertex_array(&rows, caps, profile_closed, full_turn)
  }

  /// Loft between a stack of cross-sections, each with the same point count.
  pub fn skin(sections: &[Vec<V3>], caps: Caps) -> Vnf {
    Vnf::vertex_array(sections, caps, true, false)
  }

  /// A solid between two closed 2D outlines at different heights.
  pub fn loft2d(bottom: &[V2], top: &[V2], z1: f64, z2: f64) -> Vnf {
    let lower: Vec<V3> = bottom.iter().map(|p| [p[0], p[1], z1]).collect();
    let upper: Vec<V3> = top.iter().map(|p| [p[0], p[1], z2]).collect();
    Vnf::skin(&[lower, upper], Caps::BOTH)
  }

  /// Weld vertices that share a position and drop the faces that collapse.
  ///
  /// Sweeping a profile that reaches the axis stacks a whole column of
  /// vertices on one point — a cone's tip, a sphere's poles — and a mesh
  /// that repeats a position is not manifold even though it looks closed.
  pub fn merged(&self, eps: f64) -> Vnf {
    let key = |p: &V3| {
      [
        (p[0] / eps).round() as i64,
        (p[1] / eps).round() as i64,
        (p[2] / eps).round() as i64,
      ]
    };

    let mut seen: std::collections::HashMap<[i64; 3], usize> =
      std::collections::HashMap::new();
    let mut points: Vec<V3> = Vec::with_capacity(self.points.len());
    let mut remap: Vec<usize> = Vec::with_capacity(self.points.len());
    for p in &self.points {
      let k = key(p);
      let idx = *seen.entry(k).or_insert_with(|| {
        points.push(*p);
        points.len() - 1
      });
      remap.push(idx);
    }

    let mut faces = Vec::with_capacity(self.faces.len());
    for face in &self.faces {
      let mut out: Vec<usize> = Vec::with_capacity(face.len());
      for i in face {
        let v = remap[*i];
        if out.last() != Some(&v) {
          out.push(v);
        }
      }
      // A loop that came back to where it started repeats its first vertex.
      while out.len() > 1 && out.first() == out.last() {
        out.pop();
      }
      if out.len() >= 3 {
        faces.push(out);
      }
    }

    Vnf { points, faces }
  }

  /// The mesh as a polyhedron node, flipping the winding to OpenSCAD's.
  pub fn to_node(&self) -> ScadNode {
    let merged = self.merged(1e-9);
    ScadNode::Polyhedron {
      points: merged
        .points
        .iter()
        .map(|p| [p[0] as f32, p[1] as f32, p[2] as f32])
        .collect(),
      faces: merged
        .faces
        .iter()
        .map(|f| f.iter().rev().copied().collect())
        .collect(),
    }
  }
}

/// Whether three points are collinear enough that the triangle has no area.
///
/// Sweeping a profile that touches the axis — a cone tip, a sphere pole —
/// collapses a whole column of quads onto a point, and a zero-area face
/// makes the mesh non-manifold.
fn is_degenerate(points: &[V3], tri: &[usize; 3]) -> bool {
  let a = points[tri[0]];
  let b = points[tri[1]];
  let c = points[tri[2]];
  norm(cross(sub(b, a), sub(c, a))) < 1e-12
}

/// A closed regular polygon of `n` points on a circle of radius `r`.
///
/// The first point sits on the +X axis, matching OpenSCAD's `circle()`.
pub fn circle_path(r: f64, n: u32) -> Vec<V2> {
  let n = n.max(3);
  (0..n)
    .map(|i| {
      let a = 360.0 * i as f64 / n as f64;
      let (s, c) = a.to_radians().sin_cos();
      [r * c, r * s]
    })
    .collect()
}

/// An ellipse of `n` points with the given semi-axes.
pub fn ellipse_path(rx: f64, ry: f64, n: u32) -> Vec<V2> {
  let n = n.max(3);
  (0..n)
    .map(|i| {
      let a = 360.0 * i as f64 / n as f64;
      let (s, c) = a.to_radians().sin_cos();
      [rx * c, ry * s]
    })
    .collect()
}

/// An arc of `n` points about `cp`, from `start` sweeping `sweep` degrees.
///
/// With `endpoint` the last point lands exactly on `start + sweep`; without
/// it the arc stops one step short, which is what lets several arcs be
/// concatenated into one outline without doubling up a vertex.
pub fn arc_pts(
  n: u32,
  r: f64,
  cp: V2,
  start: f64,
  sweep: f64,
  endpoint: bool,
) -> Vec<V2> {
  let n = n.max(2);
  let divisor = if endpoint { n - 1 } else { n } as f64;
  (0..n)
    .map(|i| {
      let a = start + sweep * i as f64 / divisor;
      let (s, c) = a.to_radians().sin_cos();
      [cp[0] + r * c, cp[1] + r * s]
    })
    .collect()
}

/// An arc of `segments + 1` points about the origin.
pub fn arc_path(r: f64, start: f64, sweep: f64, segments: u32) -> Vec<V2> {
  arc_pts(segments.max(1) + 1, r, [0.0, 0.0], start, sweep, true)
}

/// The signed area of a closed 2D outline; positive when counter-clockwise.
pub fn signed_area(path: &[V2]) -> f64 {
  let n = path.len();
  if n < 3 {
    return 0.0;
  }
  let mut sum = 0.0;
  for i in 0..n {
    let a = path[i];
    let b = path[(i + 1) % n];
    sum += a[0] * b[1] - b[0] * a[1];
  }
  sum / 2.0
}

/// The outline wound counter-clockwise, reversing it if needed.
pub fn ccw(path: Vec<V2>) -> Vec<V2> {
  if signed_area(&path) < 0.0 {
    path.into_iter().rev().collect()
  } else {
    path
  }
}

#[cfg(test)]
mod tests {
  use super::*;

  /// The winding a mesh comes out with decides whether Manifold sees a solid
  /// or its inside-out twin, so pin it against a shape of known volume.
  #[test]
  fn a_swept_box_has_the_volume_its_dimensions_imply() {
    let square = vec![[-5.0, -5.0], [5.0, -5.0], [5.0, 5.0], [-5.0, 5.0]];
    let vnf = Vnf::loft2d(&square, &square, 0.0, 4.0);
    let m = crate::export::materialize_scad_manifold(&vnf.to_node());
    assert!((m.volume() - 400.0).abs() < 1e-3, "{}", m.volume());
  }

  #[test]
  fn a_revolved_rectangle_makes_a_tube_of_the_expected_volume() {
    // A 2-wide, 10-tall rectangle from r=3 to r=5, revolved into a tube.
    let profile = vec![[3.0, 0.0], [5.0, 0.0], [5.0, 10.0], [3.0, 10.0]];
    let vnf = Vnf::rotate_sweep(&profile, 360.0, 128, true);
    let m = crate::export::materialize_scad_manifold(&vnf.to_node());
    let ideal = std::f64::consts::PI * (25.0 - 9.0) * 10.0;
    // A 128-gon is slightly smaller than the circle it approximates.
    let ratio = m.volume() / ideal;
    assert!(ratio > 0.999 && ratio <= 1.0, "{ratio}");
  }

  #[test]
  fn a_cone_tip_collapses_without_leaving_zero_area_faces() {
    let base = circle_path(5.0, 32);
    let tip: Vec<V2> = vec![[0.0, 0.0]; 32];
    let vnf = Vnf::loft2d(&base, &tip, 0.0, 10.0);
    let m = crate::export::materialize_scad_manifold(&vnf.to_node());
    let ideal = std::f64::consts::PI * 25.0 * 10.0 / 3.0;
    let ratio = m.volume() / ideal;
    assert!(ratio > 0.98 && ratio <= 1.0, "{ratio}");
  }

  #[test]
  fn signed_area_tells_the_two_windings_apart() {
    let square = vec![[0.0, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]];
    assert!(signed_area(&square) > 0.0);
    let flipped: Vec<V2> = square.iter().rev().copied().collect();
    assert!(signed_area(&flipped) < 0.0);
    assert_eq!(ccw(flipped), square);
  }
}
