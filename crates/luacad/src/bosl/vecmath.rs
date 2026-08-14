//! Vector and matrix helpers for the native BOSL2 implementations.
//!
//! Angles are in degrees throughout, matching OpenSCAD and BOSL2, and
//! matrices are row-major 4×4 so they drop straight into
//! [`ScadNode::Multmatrix`](crate::scad_export::ScadNode::Multmatrix).

pub type V2 = [f64; 2];
pub type V3 = [f64; 3];

/// A row-major 4×4 affine matrix.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Mat4(pub [f64; 16]);

pub const EPS: f64 = 1e-9;

// ---------------------------------------------------------------------------
// Vectors
// ---------------------------------------------------------------------------

pub fn add(a: V3, b: V3) -> V3 {
  [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

pub fn sub(a: V3, b: V3) -> V3 {
  [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

pub fn mul(a: V3, s: f64) -> V3 {
  [a[0] * s, a[1] * s, a[2] * s]
}

/// Component-wise product, BOSL2's `v_mul()`.
pub fn vmul(a: V3, b: V3) -> V3 {
  [a[0] * b[0], a[1] * b[1], a[2] * b[2]]
}

pub fn dot(a: V3, b: V3) -> f64 {
  a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

pub fn cross(a: V3, b: V3) -> V3 {
  [
    a[1] * b[2] - a[2] * b[1],
    a[2] * b[0] - a[0] * b[2],
    a[0] * b[1] - a[1] * b[0],
  ]
}

pub fn norm(v: V3) -> f64 {
  dot(v, v).sqrt()
}

/// The unit vector along `v`, or `dflt` when `v` has no length.
pub fn unit_or(v: V3, dflt: V3) -> V3 {
  let n = norm(v);
  if n < EPS { dflt } else { mul(v, 1.0 / n) }
}

pub fn lerp3(a: V3, b: V3, u: f64) -> V3 {
  [
    a[0] + (b[0] - a[0]) * u,
    a[1] + (b[1] - a[1]) * u,
    a[2] + (b[2] - a[2]) * u,
  ]
}

pub fn approx(a: f64, b: f64) -> bool {
  (a - b).abs() < 1e-9
}

pub fn approx3(a: V3, b: V3) -> bool {
  approx(a[0], b[0]) && approx(a[1], b[1]) && approx(a[2], b[2])
}

/// The angle between two vectors, in degrees.
pub fn vector_angle(a: V3, b: V3) -> f64 {
  let na = norm(a);
  let nb = norm(b);
  if na < EPS || nb < EPS {
    return 0.0;
  }
  let c = (dot(a, b) / (na * nb)).clamp(-1.0, 1.0);
  c.acos().to_degrees()
}

/// The rotation axis carrying `a` onto `b`, BOSL2's `vector_axis()`.
///
/// When the two are parallel the cross product vanishes, so a perpendicular
/// reference direction stands in — `UP` unless the vectors are themselves
/// vertical, in which case `RIGHT`.
pub fn vector_axis(a: V3, b: V3) -> V3 {
  let eps = 1e-6;
  let w1 = unit_or(a, [0.0, 0.0, 1.0]);
  let w2 = unit_or(b, [0.0, 0.0, 1.0]);
  let parallel = norm(sub(w1, w2)) <= eps || norm(add(w1, w2)) <= eps;
  let w3 = if !parallel {
    w2
  } else {
    let abs2 = [w2[0].abs(), w2[1].abs(), w2[2].abs()];
    if norm(sub(abs2, [0.0, 0.0, 1.0])) > eps {
      [0.0, 0.0, 1.0]
    } else {
      [1.0, 0.0, 0.0]
    }
  };
  unit_or(cross(w1, w3), [0.0, 1.0, 0.0])
}

// ---------------------------------------------------------------------------
// Matrices
// ---------------------------------------------------------------------------

impl Mat4 {
  pub fn identity() -> Mat4 {
    Mat4([
      1.0, 0.0, 0.0, 0.0, //
      0.0, 1.0, 0.0, 0.0, //
      0.0, 0.0, 1.0, 0.0, //
      0.0, 0.0, 0.0, 1.0,
    ])
  }

  pub fn translate(v: V3) -> Mat4 {
    Mat4([
      1.0, 0.0, 0.0, v[0], //
      0.0, 1.0, 0.0, v[1], //
      0.0, 0.0, 1.0, v[2], //
      0.0, 0.0, 0.0, 1.0,
    ])
  }

  pub fn scale(v: V3) -> Mat4 {
    Mat4([
      v[0], 0.0, 0.0, 0.0, //
      0.0, v[1], 0.0, 0.0, //
      0.0, 0.0, v[2], 0.0, //
      0.0, 0.0, 0.0, 1.0,
    ])
  }

  pub fn xrot(ang: f64) -> Mat4 {
    let (s, c) = ang.to_radians().sin_cos();
    Mat4([
      1.0, 0.0, 0.0, 0.0, //
      0.0, c, -s, 0.0, //
      0.0, s, c, 0.0, //
      0.0, 0.0, 0.0, 1.0,
    ])
  }

  pub fn yrot(ang: f64) -> Mat4 {
    let (s, c) = ang.to_radians().sin_cos();
    Mat4([
      c, 0.0, s, 0.0, //
      0.0, 1.0, 0.0, 0.0, //
      -s, 0.0, c, 0.0, //
      0.0, 0.0, 0.0, 1.0,
    ])
  }

  pub fn zrot(ang: f64) -> Mat4 {
    let (s, c) = ang.to_radians().sin_cos();
    Mat4([
      c, -s, 0.0, 0.0, //
      s, c, 0.0, 0.0, //
      0.0, 0.0, 1.0, 0.0, //
      0.0, 0.0, 0.0, 1.0,
    ])
  }

  /// Rotation of `ang` degrees about an arbitrary axis through the origin.
  pub fn rot_by_axis(axis: V3, ang: f64) -> Mat4 {
    if approx(ang, 0.0) {
      return Mat4::identity();
    }
    let u = unit_or(axis, [0.0, 0.0, 1.0]);
    let (s, c) = ang.to_radians().sin_cos();
    let c2 = 1.0 - c;
    let (x, y, z) = (u[0], u[1], u[2]);
    Mat4([
      x * x * c2 + c,
      x * y * c2 - z * s,
      x * z * c2 + y * s,
      0.0,
      y * x * c2 + z * s,
      y * y * c2 + c,
      y * z * c2 - x * s,
      0.0,
      z * x * c2 - y * s,
      z * y * c2 + x * s,
      z * z * c2 + c,
      0.0,
      0.0,
      0.0,
      0.0,
      1.0,
    ])
  }

  /// The rotation carrying direction `from` onto direction `to`.
  pub fn rot_from_to(from: V3, to: V3) -> Mat4 {
    let f = unit_or(from, [0.0, 0.0, 1.0]);
    let t = unit_or(to, [0.0, 0.0, 1.0]);
    if approx3(f, t) {
      return Mat4::identity();
    }
    Mat4::rot_by_axis(vector_axis(f, t), vector_angle(f, t))
  }

  pub fn mul(&self, other: &Mat4) -> Mat4 {
    let mut out = [0.0; 16];
    for row in 0..4 {
      for col in 0..4 {
        let mut sum = 0.0;
        for k in 0..4 {
          sum += self.0[row * 4 + k] * other.0[k * 4 + col];
        }
        out[row * 4 + col] = sum;
      }
    }
    Mat4(out)
  }

  /// Apply the matrix to a point.
  pub fn apply(&self, p: V3) -> V3 {
    let m = &self.0;
    [
      m[0] * p[0] + m[1] * p[1] + m[2] * p[2] + m[3],
      m[4] * p[0] + m[5] * p[1] + m[6] * p[2] + m[7],
      m[8] * p[0] + m[9] * p[1] + m[10] * p[2] + m[11],
    ]
  }

  pub fn is_identity(&self) -> bool {
    self
      .0
      .iter()
      .zip(Mat4::identity().0.iter())
      .all(|(a, b)| approx(*a, *b))
  }

  pub fn to_f32(self) -> [f32; 16] {
    let mut out = [0.0f32; 16];
    for (o, v) in out.iter_mut().zip(self.0.iter()) {
      *o = *v as f32;
    }
    out
  }
}

/// Rotate a point by the matrix carrying `from` onto `to`, BOSL2's
/// `rot(from=, to=, p=)`.
pub fn rot_from_to_p(from: V3, to: V3, p: V3) -> V3 {
  Mat4::rot_from_to(from, to).apply(p)
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn rot_from_to_maps_the_source_onto_the_target() {
    let m = Mat4::rot_from_to([0.0, 0.0, 1.0], [1.0, 0.0, 0.0]);
    let p = m.apply([0.0, 0.0, 1.0]);
    assert!(approx3(p, [1.0, 0.0, 0.0]), "{p:?}");
  }

  #[test]
  fn parallel_vectors_rotate_by_the_identity() {
    let m = Mat4::rot_from_to([0.0, 0.0, 1.0], [0.0, 0.0, 1.0]);
    assert!(m.is_identity());
  }

  #[test]
  fn antiparallel_vectors_flip_through_a_perpendicular_axis() {
    let m = Mat4::rot_from_to([0.0, 0.0, 1.0], [0.0, 0.0, -1.0]);
    let p = m.apply([0.0, 0.0, 1.0]);
    assert!(approx3(p, [0.0, 0.0, -1.0]), "{p:?}");
  }

  #[test]
  fn matrix_product_composes_right_to_left() {
    let m = Mat4::translate([1.0, 0.0, 0.0]).mul(&Mat4::zrot(90.0));
    let p = m.apply([1.0, 0.0, 0.0]);
    assert!(approx3(p, [1.0, 1.0, 0.0]), "{p:?}");
  }
}
