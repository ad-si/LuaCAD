//! Offsetting a closed outline, the way BOSL2's `offset()` does it.
//!
//! Shifting every edge along its own normal leaves a gap at each corner that
//! turns the wrong way — outward corners when growing, inward ones when
//! shrinking. What fills that gap is the join style: `round` sweeps an arc of
//! the offset radius, `chamfer` cuts straight across it, and `delta` runs the
//! two edges out until they meet.
//!
//! A sweep stacks many offsets of the same outline on top of each other and
//! joins consecutive ones into a surface, so every offset of a given outline
//! has to produce the same number of points in the same order. That is what
//! [`Corners`] is for: it decides once, for the whole stack, how many points
//! each corner gets, and every offset then fills exactly that many. A corner
//! that needs no arc in a particular row simply repeats its single point.

use std::f64::consts::PI;

const EPS: f64 = 1e-9;

/// How a corner is filled when an offset opens it up.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JoinStyle {
  /// An arc of the offset radius, centred on the original corner.
  Round,
  /// A single straight cut across the gap.
  Chamfer,
  /// The two offset edges run out until they intersect.
  Delta,
}

impl JoinStyle {
  pub fn parse(name: &str) -> Option<JoinStyle> {
    match name {
      "round" => Some(JoinStyle::Round),
      "chamfer" => Some(JoinStyle::Chamfer),
      "delta" => Some(JoinStyle::Delta),
      _ => None,
    }
  }
}

fn sub(a: [f64; 2], b: [f64; 2]) -> [f64; 2] {
  [a[0] - b[0], a[1] - b[1]]
}

fn cross(a: [f64; 2], b: [f64; 2]) -> f64 {
  a[0] * b[1] - a[1] * b[0]
}

fn dot(a: [f64; 2], b: [f64; 2]) -> f64 {
  a[0] * b[0] + a[1] * b[1]
}

fn unit(v: [f64; 2]) -> [f64; 2] {
  let n = (v[0] * v[0] + v[1] * v[1]).sqrt();
  if n < EPS {
    [0.0, 0.0]
  } else {
    [v[0] / n, v[1] / n]
  }
}

/// Twice the signed area, positive when the outline winds counter-clockwise.
pub fn signed_area2(path: &[[f64; 2]]) -> f64 {
  let n = path.len();
  (0..n)
    .map(|i| {
      let p = path[i];
      let q = path[(i + 1) % n];
      p[0] * q[1] - q[0] * p[1]
    })
    .sum()
}

/// The plan of one outline's corners, shared by every offset of it.
///
/// `turns[i]` is how far the outline turns at vertex `i`, signed so that a
/// positive turn is one that an outward offset has to fill. `counts[i]` is
/// how many points that corner contributes to every offset.
pub struct Corners {
  /// The outline, wound counter-clockwise.
  path: Vec<[f64; 2]>,
  /// Unit outward normal of the edge leaving each vertex.
  normals: Vec<[f64; 2]>,
  turns: Vec<f64>,
  counts: Vec<usize>,
}

impl Corners {
  /// Plan the corners of `path` for offsets of up to `max_offset` either way.
  ///
  /// `segments` is the facet count a full circle of that radius would get, so
  /// an arc filling a quarter turn gets a quarter of them.
  pub fn plan(
    path: &[[f64; 2]],
    style: JoinStyle,
    max_offset: f64,
    segments: u32,
  ) -> Corners {
    let ccw = signed_area2(path) >= 0.0;
    let path: Vec<[f64; 2]> = if ccw {
      path.to_vec()
    } else {
      path.iter().rev().copied().collect()
    };
    let n = path.len();

    // Outward normal of the edge from vertex i to vertex i+1. The outline
    // winds counter-clockwise, so its interior is to the left of every edge
    // and (dy, -dx) points away from it.
    let normals: Vec<[f64; 2]> = (0..n)
      .map(|i| {
        let d = unit(sub(path[(i + 1) % n], path[i]));
        [d[1], -d[0]]
      })
      .collect();

    let turns: Vec<f64> = (0..n)
      .map(|i| {
        let incoming = sub(path[i], path[(i + n - 1) % n]);
        let outgoing = sub(path[(i + 1) % n], path[i]);
        let a = cross(incoming, outgoing);
        let b = dot(incoming, outgoing);
        a.atan2(b)
      })
      .collect();

    let counts: Vec<usize> = turns
      .iter()
      .map(|turn| match style {
        // A straight cut and a mitre are both a single extra point, and a
        // corner that never opens up stays one point too.
        JoinStyle::Delta => 1,
        JoinStyle::Chamfer => {
          if turn.abs() < EPS {
            1
          } else {
            2
          }
        }
        JoinStyle::Round => {
          if turn.abs() < EPS || max_offset.abs() < EPS {
            1
          } else {
            // BOSL2 gives a corner `1 + floor(segs(r)·angle/360)` steps and
            // then draws it with three points more than that, so even the
            // shallowest corner is a real arc rather than a single chord.
            let share = turn.abs() / (2.0 * PI);
            let steps = 1 + (segments as f64 * share).floor() as usize;
            (steps + 3).min(128)
          }
        }
      })
      .collect();

    Corners {
      path,
      normals,
      turns,
      counts,
    }
  }

  /// How many points every offset of this outline produces.
  pub fn point_count(&self) -> usize {
    self.counts.iter().sum()
  }

  pub fn is_empty(&self) -> bool {
    self.path.len() < 3
  }

  /// The outline offset outward by `d`, negative to shrink it.
  ///
  /// The result always has [`point_count`](Self::point_count) points, so two
  /// offsets of the same outline correspond point for point and can be joined
  /// into a surface directly.
  pub fn offset(&self, d: f64, style: JoinStyle) -> Vec<[f64; 2]> {
    let n = self.path.len();
    let mut out = Vec::with_capacity(self.point_count());
    for i in 0..n {
      let here = self.path[i];
      let n_in = self.normals[(i + n - 1) % n];
      let n_out = self.normals[i];
      let count = self.counts[i];
      let turn = self.turns[i];

      // The corner only opens up when the turn and the offset agree in sign;
      // otherwise the two offset edges overlap and meeting them is right.
      let opens = d * turn > EPS * EPS;
      if !opens || style == JoinStyle::Delta || count < 2 {
        let p = self.mitre(i, d);
        for _ in 0..count {
          out.push(p);
        }
        continue;
      }

      if style == JoinStyle::Chamfer {
        // Straight across the gap: the first and last points, and repeats of
        // the first in between to keep the corner's share of the row.
        let start = [here[0] + n_in[0] * d, here[1] + n_in[1] * d];
        let end = [here[0] + n_out[0] * d, here[1] + n_out[1] * d];
        for _ in 0..count - 1 {
          out.push(start);
        }
        out.push(end);
        continue;
      }

      // An arc of the offset radius about the corner, turning from the
      // incoming edge's normal to the outgoing one's.
      let a0 = n_in[1].atan2(n_in[0]);
      for k in 0..count {
        let u = k as f64 / (count - 1) as f64;
        let a = a0 + turn * u;
        out.push([here[0] + d * a.cos(), here[1] + d * a.sin()]);
      }
    }
    out
  }

  /// Where the two offset edges either side of vertex `i` meet.
  fn mitre(&self, i: usize, d: f64) -> [f64; 2] {
    let n = self.path.len();
    let here = self.path[i];
    let n_in = self.normals[(i + n - 1) % n];
    let n_out = self.normals[i];
    let bisector = [n_in[0] + n_out[0], n_in[1] + n_out[1]];
    let b = unit(bisector);
    if b == [0.0, 0.0] {
      // A perfect reversal — the edges double back, so there is no corner to
      // move and the vertex stays where it is.
      return here;
    }
    // The corner travels further than the edges by however sharp it is,
    // clamped so a near-spike does not fly off to infinity.
    let cosang = dot(b, n_in).abs().max(0.1);
    [here[0] + b[0] * d / cosang, here[1] + b[1] * d / cosang]
  }

  /// Whether shrinking by `d` has folded the outline over on itself.
  ///
  /// An offset point should sit exactly `|d|` away from the outline it came
  /// from. One that ends up closer has crossed to the far side of some other
  /// edge, which means the outline has collapsed and the sweep built on it
  /// would be self-intersecting.
  pub fn is_valid(&self, d: f64) -> bool {
    if d.abs() < EPS {
      return true;
    }
    let pts = self.offset(d, JoinStyle::Round);
    let limit = d.abs() * (1.0 - 1e-4);
    pts.iter().all(|p| self.distance_to(*p) >= limit)
  }

  /// The shortest distance from a point to the outline itself.
  fn distance_to(&self, p: [f64; 2]) -> f64 {
    let n = self.path.len();
    (0..n)
      .map(|i| point_segment_distance(p, self.path[i], self.path[(i + 1) % n]))
      .fold(f64::INFINITY, f64::min)
  }
}

fn point_segment_distance(p: [f64; 2], a: [f64; 2], b: [f64; 2]) -> f64 {
  let ab = sub(b, a);
  let len2 = dot(ab, ab);
  let t = if len2 < EPS {
    0.0
  } else {
    (dot(sub(p, a), ab) / len2).clamp(0.0, 1.0)
  };
  let q = [a[0] + ab[0] * t, a[1] + ab[1] * t];
  let d = sub(p, q);
  dot(d, d).sqrt()
}

#[cfg(test)]
mod tests {
  use super::*;

  fn square(s: f64) -> Vec<[f64; 2]> {
    vec![[-s, -s], [s, -s], [s, s], [-s, s]]
  }

  fn bounds(pts: &[[f64; 2]]) -> ([f64; 2], [f64; 2]) {
    let mut lo = [f64::INFINITY; 2];
    let mut hi = [f64::NEG_INFINITY; 2];
    for p in pts {
      for k in 0..2 {
        lo[k] = lo[k].min(p[k]);
        hi[k] = hi[k].max(p[k]);
      }
    }
    (lo, hi)
  }

  #[test]
  fn growing_a_square_rounds_its_corners() {
    let c = Corners::plan(&square(10.0), JoinStyle::Round, 2.0, 32);
    let out = c.offset(2.0, JoinStyle::Round);
    let (lo, hi) = bounds(&out);
    assert!((hi[0] - 12.0).abs() < 1e-6, "{hi:?}");
    assert!((lo[0] - -12.0).abs() < 1e-6, "{lo:?}");
    // The corner is an arc, so no point reaches the mitred [12, 12].
    assert!(
      out
        .iter()
        .all(|p| p[0].hypot(p[1]) < 10.0 * 2f64.sqrt() + 2.0 + 1e-6),
      "{out:?}"
    );
    let corner = out
      .iter()
      .any(|p| (p[0] - 12.0).abs() < 1e-6 && (p[1] - 12.0).abs() < 1e-6);
    assert!(!corner, "a round join should not reach the mitre point");
  }

  #[test]
  fn a_delta_offset_mitres_the_corners_instead() {
    let c = Corners::plan(&square(10.0), JoinStyle::Delta, 2.0, 32);
    let out = c.offset(2.0, JoinStyle::Delta);
    assert_eq!(out.len(), 4);
    let (lo, hi) = bounds(&out);
    assert!(
      (hi[0] - 12.0).abs() < 1e-6 && (hi[1] - 12.0).abs() < 1e-6,
      "{hi:?}"
    );
    assert!((lo[0] - -12.0).abs() < 1e-6, "{lo:?}");
  }

  #[test]
  fn every_offset_of_one_outline_has_the_same_point_count() {
    let c = Corners::plan(&square(10.0), JoinStyle::Round, 3.0, 32);
    let n = c.point_count();
    for d in [-3.0, -1.0, 0.0, 1.0, 3.0] {
      assert_eq!(c.offset(d, JoinStyle::Round).len(), n, "at d = {d}");
    }
  }

  #[test]
  fn shrinking_keeps_the_corners_sharp() {
    let c = Corners::plan(&square(10.0), JoinStyle::Round, 2.0, 32);
    let out = c.offset(-2.0, JoinStyle::Round);
    let (lo, hi) = bounds(&out);
    assert!((hi[0] - 8.0).abs() < 1e-6, "{hi:?}");
    assert!((hi[1] - 8.0).abs() < 1e-6, "{hi:?}");
    assert!((lo[0] - -8.0).abs() < 1e-6, "{lo:?}");
  }

  #[test]
  fn winding_does_not_change_the_result() {
    let cw: Vec<[f64; 2]> = square(10.0).iter().rev().copied().collect();
    let a = Corners::plan(&square(10.0), JoinStyle::Round, 2.0, 32)
      .offset(2.0, JoinStyle::Round);
    let b = Corners::plan(&cw, JoinStyle::Round, 2.0, 32)
      .offset(2.0, JoinStyle::Round);
    let (alo, ahi) = bounds(&a);
    let (blo, bhi) = bounds(&b);
    for k in 0..2 {
      assert!((alo[k] - blo[k]).abs() < 1e-6);
      assert!((ahi[k] - bhi[k]).abs() < 1e-6);
    }
  }

  #[test]
  fn shrinking_past_the_middle_is_rejected() {
    let c = Corners::plan(&square(5.0), JoinStyle::Round, 8.0, 32);
    assert!(c.is_valid(-2.0));
    // The square is 10 across, so taking 8 off every side folds it over.
    assert!(!c.is_valid(-8.0));
  }

  #[test]
  fn a_reflex_corner_rounds_when_the_outline_shrinks() {
    // An L, so the inner corner turns the other way from the rest.
    let l = vec![
      [0.0, 0.0],
      [10.0, 0.0],
      [10.0, 4.0],
      [4.0, 4.0],
      [4.0, 10.0],
      [0.0, 10.0],
    ];
    let c = Corners::plan(&l, JoinStyle::Round, 1.0, 32);
    // Vertex [4,4] is the reflex one; shrinking opens it up.
    let out = c.offset(-1.0, JoinStyle::Round);
    let near: Vec<_> = out
      .iter()
      .filter(|p| (p[0] - 4.0).abs() < 2.0 && (p[1] - 4.0).abs() < 2.0)
      .collect();
    assert!(
      near.len() > 2,
      "expected an arc at the reflex corner: {near:?}"
    );
  }
}
