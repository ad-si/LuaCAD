//! BOSL2's `geometry.scad`: lines, planes, circles, polygons and hulls.
//!
//! A line is two points, a plane is `[A, B, C, D]` with `A·x + B·y + C·z = D`
//! and the normal `[A,B,C]` scaled to unit length, and a polygon is a list of
//! points with no repeated closing vertex. Indices returned into a list count
//! from zero, matching OpenSCAD.

use mlua::{Lua, Result as LuaResult, Value as LuaValue};

use crate::bosl::value::{
  Args, PureFn, Val, matrix, num_list, register_all, v2, v3,
};

const EPS: f64 = 1e-9;

// ---------------------------------------------------------------------------
// Small vector helpers
// ---------------------------------------------------------------------------

fn sub(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
  [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn add(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
  [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

fn scale(a: [f64; 3], k: f64) -> [f64; 3] {
  [a[0] * k, a[1] * k, a[2] * k]
}

fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
  a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
  [
    a[1] * b[2] - a[2] * b[1],
    a[2] * b[0] - a[0] * b[2],
    a[0] * b[1] - a[1] * b[0],
  ]
}

fn norm(a: [f64; 3]) -> f64 {
  dot(a, a).sqrt()
}

fn unit(a: [f64; 3]) -> Option<[f64; 3]> {
  let n = norm(a);
  if n < EPS {
    None
  } else {
    Some(scale(a, 1.0 / n))
  }
}

/// The z component of the 2D cross product, positive when `b` turns left.
fn cross2(a: [f64; 2], b: [f64; 2]) -> f64 {
  a[0] * b[1] - a[1] * b[0]
}

/// The two endpoints of a line parameter.
fn line_of(a: &Args, name: &str) -> LuaResult<([f64; 3], [f64; 3])> {
  let m = a.need_matrix(name)?;
  if m.len() != 2 {
    return a.err(format!("{name} must be two points"));
  }
  Ok((v3(&m[0]), v3(&m[1])))
}

/// Read a `bounded` flag, which may be one boolean or one per end.
fn bounds(a: &Args, name: &str) -> [bool; 2] {
  match a.val(name) {
    Some(Val::Num(n)) => [n != 0.0; 2],
    Some(Val::List(items)) => {
      let f = |i: usize| {
        items
          .get(i)
          .and_then(|v| v.as_num())
          .is_some_and(|n| n != 0.0)
      };
      [f(0), f(1)]
    }
    None => [false; 2],
  }
}

// ---------------------------------------------------------------------------
// Lines
// ---------------------------------------------------------------------------

/// How far `pt` is along `p1`–`p2`, as a fraction of the segment.
fn line_param(p1: [f64; 3], p2: [f64; 3], pt: [f64; 3]) -> f64 {
  let d = sub(p2, p1);
  let len2 = dot(d, d);
  if len2 < EPS {
    0.0
  } else {
    dot(sub(pt, p1), d) / len2
  }
}

fn point_line_distance(_lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let (p1, p2) = line_of(a, "line")?;
  let pt = v3(&a.need_vec("pt")?);
  let b = bounds(a, "bounded");
  let mut t = line_param(p1, p2, pt);
  if b[0] {
    t = t.max(0.0);
  }
  if b[1] {
    t = t.min(1.0);
  }
  let closest = add(p1, scale(sub(p2, p1), t));
  Ok(LuaValue::Number(norm(sub(pt, closest))))
}

fn is_point_on_line(_lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let (p1, p2) = line_of(a, "line")?;
  let pt = v3(&a.need_vec("point")?);
  let eps = a.num_or("eps", EPS);
  let b = bounds(a, "bounded");
  let t = line_param(p1, p2, pt);
  let closest = add(p1, scale(sub(p2, p1), t));
  let on = norm(sub(pt, closest)) <= eps
    && (!b[0] || t >= -eps)
    && (!b[1] || t <= 1.0 + eps);
  Ok(LuaValue::Boolean(on))
}

fn is_collinear(_lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let eps = a.num_or("eps", EPS);
  // Either three points, or one list of points.
  let pts: Vec<[f64; 3]> = match (a.val("p1"), a.val("p2"), a.val("p3")) {
    (Some(v), None, None) => {
      let Some(m) = v.as_matrix() else {
        return a.err("give three points, or a list of points");
      };
      m.iter().map(|p| v3(p)).collect()
    }
    (Some(p1), Some(p2), Some(p3)) => {
      let get = |v: &Val| v.as_vec().map(|p| v3(&p));
      match (get(&p1), get(&p2), get(&p3)) {
        (Some(a), Some(b), Some(c)) => vec![a, b, c],
        _ => return a.err("give three points, or a list of points"),
      }
    }
    _ => return a.err("give three points, or a list of points"),
  };
  if pts.len() < 3 {
    return Ok(LuaValue::Boolean(true));
  }
  // Every point has to lie on the line through the two furthest apart, which
  // is the pair that gives the best-conditioned line to test against.
  let mut best = (0usize, 1usize, 0.0f64);
  for i in 0..pts.len() {
    for j in (i + 1)..pts.len() {
      let d = norm(sub(pts[j], pts[i]));
      if d > best.2 {
        best = (i, j, d);
      }
    }
  }
  if best.2 < eps {
    return Ok(LuaValue::Boolean(true));
  }
  let (p1, p2) = (pts[best.0], pts[best.1]);
  let ok = pts.iter().all(|p| {
    let t = line_param(p1, p2, *p);
    norm(sub(*p, add(p1, scale(sub(p2, p1), t)))) <= eps
  });
  Ok(LuaValue::Boolean(ok))
}

fn line_normal(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  // Either two points, or one line.
  let (p1, p2) = match (a.val("p1"), a.val("p2")) {
    (Some(l), None) => {
      let Some(m) = l.as_matrix() else {
        return a.err("give a line, or two points");
      };
      if m.len() != 2 {
        return a.err("give a line, or two points");
      }
      (v2(&m[0]), v2(&m[1]))
    }
    (Some(x), Some(y)) => {
      let (Some(x), Some(y)) = (x.as_vec(), y.as_vec()) else {
        return a.err("give a line, or two points");
      };
      (v2(&x), v2(&y))
    }
    _ => return a.err("give a line, or two points"),
  };
  let n = [p1[1] - p2[1], p2[0] - p1[0]];
  let len = (n[0] * n[0] + n[1] * n[1]).sqrt();
  if len < EPS {
    return a.err("the two points are the same");
  }
  num_list(lua, &[n[0] / len, n[1] / len])
}

fn line_from_points(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let pts = a.need_matrix("points")?;
  if pts.len() < 2 {
    return a.err("at least two points are needed");
  }
  // The line through the two points furthest apart best represents the set.
  let mut best = (0usize, 1usize, 0.0f64);
  for i in 0..pts.len() {
    for j in (i + 1)..pts.len() {
      let d = norm(sub(v3(&pts[j]), v3(&pts[i])));
      if d > best.2 {
        best = (i, j, d);
      }
    }
  }
  if best.2 < a.num_or("eps", EPS) {
    return Ok(LuaValue::Nil);
  }
  matrix(lua, &[pts[best.0].clone(), pts[best.1].clone()])
}

/// Where two 2D lines meet, and how far along each the meeting point lies.
fn intersect2(
  a1: [f64; 2],
  a2: [f64; 2],
  b1: [f64; 2],
  b2: [f64; 2],
) -> Option<([f64; 2], f64, f64)> {
  let d1 = [a2[0] - a1[0], a2[1] - a1[1]];
  let d2 = [b2[0] - b1[0], b2[1] - b1[1]];
  let denom = cross2(d1, d2);
  if denom.abs() < EPS {
    return None;
  }
  let diff = [b1[0] - a1[0], b1[1] - a1[1]];
  let t = cross2(diff, d2) / denom;
  let u = cross2(diff, d1) / denom;
  Some(([a1[0] + d1[0] * t, a1[1] + d1[1] * t], t, u))
}

fn line_intersection(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let l1 = a.need_matrix("line1")?;
  let l2 = a.need_matrix("line2")?;
  if l1.len() != 2 || l2.len() != 2 {
    return a.err("each line must be two points");
  }
  let eps = a.num_or("eps", EPS);
  let shared = bounds(a, "bounded");
  let b1 = if a.has("bounded1") {
    bounds(a, "bounded1")
  } else {
    shared
  };
  let b2 = if a.has("bounded2") {
    bounds(a, "bounded2")
  } else {
    shared
  };

  let Some((pt, t, u)) =
    intersect2(v2(&l1[0]), v2(&l1[1]), v2(&l2[0]), v2(&l2[1]))
  else {
    return Ok(LuaValue::Nil);
  };
  let inside = |bounded: [bool; 2], s: f64| {
    (!bounded[0] || s >= -eps) && (!bounded[1] || s <= 1.0 + eps)
  };
  if inside(b1, t) && inside(b2, u) {
    num_list(lua, &pt)
  } else {
    Ok(LuaValue::Nil)
  }
}

fn line_closest_point(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let m = a.need_matrix("line")?;
  if m.len() != 2 {
    return a.err("line must be two points");
  }
  let dim = m[0].len().max(m[1].len());
  let (p1, p2) = (v3(&m[0]), v3(&m[1]));
  let pt = v3(&a.need_vec("pt")?);
  let b = bounds(a, "bounded");
  let mut t = line_param(p1, p2, pt);
  if b[0] {
    t = t.max(0.0);
  }
  if b[1] {
    t = t.min(1.0);
  }
  let closest = add(p1, scale(sub(p2, p1), t));
  num_list(lua, &closest[..dim.min(3)])
}

fn segment_distance(_lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let (p1, p2) = line_of(a, "seg1")?;
  let (q1, q2) = line_of(a, "seg2")?;
  Ok(LuaValue::Number(segment_gap(p1, p2, q1, q2)))
}

/// The shortest distance between two segments in 3D.
fn segment_gap(p1: [f64; 3], p2: [f64; 3], q1: [f64; 3], q2: [f64; 3]) -> f64 {
  let u = sub(p2, p1);
  let v = sub(q2, q1);
  let w = sub(p1, q1);
  let (a, b, c) = (dot(u, u), dot(u, v), dot(v, v));
  let (d, e) = (dot(u, w), dot(v, w));
  let denom = a * c - b * b;
  // Parallel segments have no unique closest pair, so one end is pinned and
  // the other solved for.
  let mut sc = if denom.abs() < EPS {
    0.0
  } else {
    (b * e - c * d) / denom
  };
  sc = sc.clamp(0.0, 1.0);
  // Clamping one parameter moves the other, so both are settled in turn.
  let tc = ((e + b * sc) / c.max(EPS)).clamp(0.0, 1.0);
  sc = ((b * tc - d) / a.max(EPS)).clamp(0.0, 1.0);
  let dp = sub(add(w, scale(u, sc)), scale(v, tc));
  norm(dp)
}

// ---------------------------------------------------------------------------
// Planes
// ---------------------------------------------------------------------------

/// A plane as `[A, B, C, D]`, with the normal already unit length.
fn plane_of(normal: [f64; 3], point: [f64; 3]) -> Option<[f64; 4]> {
  let n = unit(normal)?;
  Some([n[0], n[1], n[2], dot(n, point)])
}

fn read_plane(a: &Args, name: &str) -> LuaResult<[f64; 4]> {
  let v = a.need_vec(name)?;
  if v.len() != 4 {
    return a.err(format!("{name} must be a plane, as [A, B, C, D]"));
  }
  Ok([v[0], v[1], v[2], v[3]])
}

fn plane_through(pts: &[[f64; 3]]) -> Option<[f64; 4]> {
  if pts.len() < 3 {
    return None;
  }
  // Newell's method: the summed edge cross products give a normal that
  // averages over the whole outline rather than trusting any three points.
  let mut n = [0.0; 3];
  for i in 0..pts.len() {
    let p = pts[i];
    let q = pts[(i + 1) % pts.len()];
    n[0] += (p[1] - q[1]) * (p[2] + q[2]);
    n[1] += (p[2] - q[2]) * (p[0] + q[0]);
    n[2] += (p[0] - q[0]) * (p[1] + q[1]);
  }
  plane_of(n, pts[0])
}

fn plane3pt(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let pts: Vec<[f64; 3]> = match (a.val("p1"), a.val("p2"), a.val("p3")) {
    (Some(v), None, None) => {
      let Some(m) = v.as_matrix() else {
        return a.err("give three points, or a list of three");
      };
      m.iter().map(|p| v3(p)).collect()
    }
    (Some(p1), Some(p2), Some(p3)) => {
      let get = |v: &Val| v.as_vec().map(|p| v3(&p));
      match (get(&p1), get(&p2), get(&p3)) {
        (Some(x), Some(y), Some(z)) => vec![x, y, z],
        _ => return a.err("give three points"),
      }
    }
    _ => return a.err("give three points"),
  };
  if pts.len() < 3 {
    return a.err("three points are needed");
  }
  // BOSL2 takes the normal as cross(p3 - p1, p2 - p1).
  match plane_of(sub(pts[2], pts[0]), pts[0]).and_then(|_| {
    plane_of(cross(sub(pts[2], pts[0]), sub(pts[1], pts[0])), pts[0])
  }) {
    Some(p) => num_list(lua, &p),
    None => Ok(LuaValue::Nil),
  }
}

fn plane3pt_indexed(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let pts = a.need_points3("points")?;
  let idx = [
    a.need_num("i1")? as usize,
    a.need_num("i2")? as usize,
    a.need_num("i3")? as usize,
  ];
  let chosen: Vec<[f64; 3]> =
    idx.iter().filter_map(|i| pts.get(*i).copied()).collect();
  if chosen.len() != 3 {
    return a.err("the indices must be inside the point list");
  }
  match plane_of(
    cross(sub(chosen[2], chosen[0]), sub(chosen[1], chosen[0])),
    chosen[0],
  ) {
    Some(p) => num_list(lua, &p),
    None => Ok(LuaValue::Nil),
  }
}

fn plane_from_normal(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let n = v3(&a.need_vec("normal")?);
  let pt = a.val("pt").and_then(|v| v.as_vec()).map(|p| v3(&p));
  match plane_of(n, pt.unwrap_or([0.0; 3])) {
    Some(p) => num_list(lua, &p),
    None => a.err("the normal cannot be zero"),
  }
}

fn plane_from_points(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let pts = a.need_points3("points")?;
  if pts.len() < 3 {
    return a.err("at least three points are needed");
  }
  // Fit the plane through the centroid, using the direction of least spread
  // as the normal, so a cloud with noise still gives a sensible answer.
  let n = pts.len() as f64;
  let centre = scale(pts.iter().fold([0.0; 3], |acc, p| add(acc, *p)), 1.0 / n);
  let mut cov = [[0.0f64; 3]; 3];
  for p in &pts {
    let d = sub(*p, centre);
    for i in 0..3 {
      for j in 0..3 {
        cov[i][j] += d[i] * d[j];
      }
    }
  }
  // The smallest eigenvector of the covariance, found by inverse iteration
  // on the cross products of the rows.
  let candidates = [
    cross(
      [cov[0][0], cov[0][1], cov[0][2]],
      [cov[1][0], cov[1][1], cov[1][2]],
    ),
    cross(
      [cov[1][0], cov[1][1], cov[1][2]],
      [cov[2][0], cov[2][1], cov[2][2]],
    ),
    cross(
      [cov[0][0], cov[0][1], cov[0][2]],
      [cov[2][0], cov[2][1], cov[2][2]],
    ),
  ];
  let best = candidates
    .iter()
    .max_by(|a, b| norm(**a).total_cmp(&norm(**b)));
  match best.and_then(|n| plane_of(*n, centre)) {
    Some(p) => num_list(lua, &p),
    None => Ok(LuaValue::Nil),
  }
}

fn plane_from_polygon(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let pts = a.need_points3("poly")?;
  match plane_through(&pts) {
    Some(p) => num_list(lua, &p),
    None => Ok(LuaValue::Nil),
  }
}

fn plane_normal(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let p = read_plane(a, "plane")?;
  match unit([p[0], p[1], p[2]]) {
    Some(n) => num_list(lua, &n),
    None => a.err("the plane's normal is zero"),
  }
}

fn plane_offset(_lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let p = read_plane(a, "plane")?;
  let n = norm([p[0], p[1], p[2]]);
  if n < EPS {
    return a.err("the plane's normal is zero");
  }
  Ok(LuaValue::Number(p[3] / n))
}

fn point_plane_distance(_lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let p = read_plane(a, "plane")?;
  let pt = v3(&a.need_vec("point")?);
  let n = norm([p[0], p[1], p[2]]);
  if n < EPS {
    return a.err("the plane's normal is zero");
  }
  Ok(LuaValue::Number((dot([p[0], p[1], p[2]], pt) - p[3]) / n))
}

fn plane_closest_point(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let p = read_plane(a, "plane")?;
  let Some(n) = unit([p[0], p[1], p[2]]) else {
    return a.err("the plane's normal is zero");
  };
  let offset = p[3] / norm([p[0], p[1], p[2]]);
  let project = |pt: [f64; 3]| sub(pt, scale(n, dot(n, pt) - offset));
  let target = a.need_val("points")?;
  match target.as_vec() {
    Some(pt) if pt.len() == 3 => num_list(lua, &project(v3(&pt))),
    _ => {
      let Some(pts) = target.as_matrix() else {
        return a.err("points must be a point or a list of points");
      };
      Val::list(pts.iter().map(|pt| Val::vec(project(v3(pt))))).to_lua(lua)
    }
  }
}

fn plane_line_intersection(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let p = read_plane(a, "plane")?;
  let (l1, l2) = line_of(a, "line")?;
  let b = bounds(a, "bounded");
  let eps = a.num_or("eps", EPS);
  let n = [p[0], p[1], p[2]];
  let d = sub(l2, l1);
  let denom = dot(n, d);
  if denom.abs() < eps {
    // A line in the plane meets it everywhere; one off it, nowhere.
    return Ok(LuaValue::Nil);
  }
  let t = (p[3] - dot(n, l1)) / denom;
  if (b[0] && t < -eps) || (b[1] && t > 1.0 + eps) {
    return Ok(LuaValue::Nil);
  }
  num_list(lua, &add(l1, scale(d, t)))
}

fn plane_intersection(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let p1 = read_plane(a, "plane1")?;
  let p2 = read_plane(a, "plane2")?;
  let n1 = [p1[0], p1[1], p1[2]];
  let n2 = [p2[0], p2[1], p2[2]];

  if let Ok(p3) = read_plane(a, "plane3") {
    // Three planes meet at a point, unless two of them are parallel.
    let n3 = [p3[0], p3[1], p3[2]];
    let det = dot(n1, cross(n2, n3));
    if det.abs() < EPS {
      return Ok(LuaValue::Nil);
    }
    let pt = scale(
      add(
        add(scale(cross(n2, n3), p1[3]), scale(cross(n3, n1), p2[3])),
        scale(cross(n1, n2), p3[3]),
      ),
      1.0 / det,
    );
    return num_list(lua, &pt);
  }

  // Two planes meet along a line, returned as two points on it.
  let dir = cross(n1, n2);
  let Some(dir) = unit(dir) else {
    return Ok(LuaValue::Nil);
  };
  let det = dot(dir, dir);
  let pt = scale(
    add(scale(cross(n2, dir), p1[3]), scale(cross(dir, n1), p2[3])),
    1.0 / det,
  );
  matrix(lua, &[pt.to_vec(), add(pt, dir).to_vec()])
}

fn plane_line_angle(_lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let p = read_plane(a, "plane")?;
  let (l1, l2) = line_of(a, "line")?;
  let (Some(n), Some(d)) = (unit([p[0], p[1], p[2]]), unit(sub(l2, l1))) else {
    return a.err("the plane normal and the line must both be non-degenerate");
  };
  // Measured from the plane, not from its normal, so a line lying in the
  // plane reads as zero.
  Ok(LuaValue::Number(
    dot(n, d).clamp(-1.0, 1.0).asin().to_degrees(),
  ))
}

fn is_coplanar(_lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let pts = a.need_points3("points")?;
  let eps = a.num_or("eps", EPS);
  if pts.len() < 4 {
    return Ok(LuaValue::Boolean(true));
  }
  match plane_through(&pts) {
    None => Ok(LuaValue::Boolean(true)),
    Some(p) => {
      let n = [p[0], p[1], p[2]];
      Ok(LuaValue::Boolean(
        pts.iter().all(|pt| (dot(n, *pt) - p[3]).abs() <= eps),
      ))
    }
  }
}

fn are_points_on_plane(_lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let pts = a.need_points3("points")?;
  let p = read_plane(a, "plane")?;
  let eps = a.num_or("eps", EPS);
  let n = [p[0], p[1], p[2]];
  Ok(LuaValue::Boolean(
    pts.iter().all(|pt| (dot(n, *pt) - p[3]).abs() <= eps),
  ))
}

// ---------------------------------------------------------------------------
// Circles and spheres
// ---------------------------------------------------------------------------

fn circle_line_intersection(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let r = match a.radius("r", "d", None) {
    Some(r) => r,
    None => return a.err("give a radius or a diameter"),
  };
  let cp = a
    .val("cp")
    .and_then(|v| v.as_vec())
    .map(|p| v2(&p))
    .unwrap_or([0.0, 0.0]);
  let m = a.need_matrix("line")?;
  if m.len() != 2 {
    return a.err("line must be two points");
  }
  let (p1, p2) = (v2(&m[0]), v2(&m[1]));
  let b = bounds(a, "bounded");
  let eps = a.num_or("eps", EPS);

  let d = [p2[0] - p1[0], p2[1] - p1[1]];
  let f = [p1[0] - cp[0], p1[1] - cp[1]];
  let qa = d[0] * d[0] + d[1] * d[1];
  let qb = 2.0 * (f[0] * d[0] + f[1] * d[1]);
  let qc = f[0] * f[0] + f[1] * f[1] - r * r;
  let disc = qb * qb - 4.0 * qa * qc;
  if qa < eps || disc < 0.0 {
    return Val::List(vec![]).to_lua(lua);
  }
  let root = disc.sqrt();
  let ts = [(-qb - root) / (2.0 * qa), (-qb + root) / (2.0 * qa)];
  let hits: Vec<Val> = ts
    .iter()
    .filter(|t| (!b[0] || **t >= -eps) && (!b[1] || **t <= 1.0 + eps))
    .map(|t| Val::vec([p1[0] + d[0] * t, p1[1] + d[1] * t]))
    .collect();
  Val::List(hits).to_lua(lua)
}

fn circle_circle_intersection(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let r1 = match a.radius("r1", "d1", None) {
    Some(r) => r,
    None => return a.err("give r1 or d1"),
  };
  let r2 = match a.radius("r2", "d2", None) {
    Some(r) => r,
    None => return a.err("give r2 or d2"),
  };
  let c1 = v2(&a.need_vec("cp1")?);
  let c2 = v2(&a.need_vec("cp2")?);
  let eps = a.num_or("eps", EPS);

  let d = [c2[0] - c1[0], c2[1] - c1[1]];
  let dist = (d[0] * d[0] + d[1] * d[1]).sqrt();
  if dist < eps || dist > r1 + r2 + eps || dist < (r1 - r2).abs() - eps {
    return Val::List(vec![]).to_lua(lua);
  }
  let x = (dist * dist + r1 * r1 - r2 * r2) / (2.0 * dist);
  let h2 = r1 * r1 - x * x;
  let h = if h2 <= 0.0 { 0.0 } else { h2.sqrt() };
  let base = [c1[0] + d[0] * x / dist, c1[1] + d[1] * x / dist];
  let off = [-d[1] * h / dist, d[0] * h / dist];
  if h < eps {
    return Val::list([Val::vec(base)]).to_lua(lua);
  }
  Val::list([
    Val::vec([base[0] + off[0], base[1] + off[1]]),
    Val::vec([base[0] - off[0], base[1] - off[1]]),
  ])
  .to_lua(lua)
}

fn circle_2tangents(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let r = match a.radius("r", "d", None) {
    Some(r) => r,
    None => return a.err("give a radius or a diameter"),
  };
  let pts: Vec<[f64; 3]> = match (a.val("pt1"), a.val("pt2"), a.val("pt3")) {
    (Some(v), None, None) => {
      let Some(m) = v.as_matrix() else {
        return a.err("give three points");
      };
      m.iter().map(|p| v3(p)).collect()
    }
    (Some(p1), Some(p2), Some(p3)) => {
      let get = |v: &Val| v.as_vec().map(|p| v3(&p));
      match (get(&p1), get(&p2), get(&p3)) {
        (Some(x), Some(y), Some(z)) => vec![x, y, z],
        _ => return a.err("give three points"),
      }
    }
    _ => return a.err("give three points"),
  };
  if pts.len() < 3 {
    return a.err("three points are needed");
  }
  // The circle sits along the bisector of the corner, back far enough that
  // it just touches both legs.
  let (Some(v1), Some(v2)) =
    (unit(sub(pts[0], pts[1])), unit(sub(pts[2], pts[1])))
  else {
    return Ok(LuaValue::Nil);
  };
  let Some(mid) = unit(add(v1, v2)) else {
    return Ok(LuaValue::Nil);
  };
  let half = dot(v1, mid).clamp(-1.0, 1.0).acos();
  if half.abs() < EPS {
    return Ok(LuaValue::Nil);
  }
  let cp = add(pts[1], scale(mid, r / half.sin()));
  let tan_len = r / half.tan();
  let t1 = add(pts[1], scale(v1, tan_len));
  let t2 = add(pts[1], scale(v2, tan_len));
  let n = unit(cross(v1, v2)).unwrap_or([0.0, 0.0, 1.0]);

  let dim = a.need_matrix("pt1").map(|m| m[0].len()).unwrap_or(3);
  let trim = |p: [f64; 3]| Val::vec(p[..dim.min(3)].to_vec());
  if a.bool_or("tangents", false) {
    Val::list([trim(cp), Val::vec(n), trim(t1), trim(t2)]).to_lua(lua)
  } else {
    Val::list([trim(cp), Val::Num(r), Val::vec(n)]).to_lua(lua)
  }
}

fn circle_3points(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let pts: Vec<[f64; 3]> = match (a.val("pt1"), a.val("pt2"), a.val("pt3")) {
    (Some(v), None, None) => {
      let Some(m) = v.as_matrix() else {
        return a.err("give three points");
      };
      m.iter().map(|p| v3(p)).collect()
    }
    (Some(p1), Some(p2), Some(p3)) => {
      let get = |v: &Val| v.as_vec().map(|p| v3(&p));
      match (get(&p1), get(&p2), get(&p3)) {
        (Some(x), Some(y), Some(z)) => vec![x, y, z],
        _ => return a.err("give three points"),
      }
    }
    _ => return a.err("give three points"),
  };
  if pts.len() < 3 {
    return a.err("three points are needed");
  }
  // A 2D question gets a 2D answer, so the input's own width decides.
  let input_dim = match (a.val("pt1"), a.val("pt2")) {
    (Some(v), None) => v
      .as_matrix()
      .and_then(|m| m.first().map(|p| p.len()))
      .unwrap_or(3),
    (Some(v), Some(_)) => v.as_vec().map(|p| p.len()).unwrap_or(3),
    _ => 3,
  };
  let (p1, p2, p3) = (pts[0], pts[1], pts[2]);
  let e1 = sub(p2, p1);
  let e2 = sub(p3, p1);
  let n = cross(e1, e2);
  if norm(n) < EPS {
    return Val::list([Val::Num(f64::NAN); 0]).to_lua(lua);
  }
  // The centre is where the two perpendicular bisectors meet, written
  // directly rather than by solving a system.
  let denom = 2.0 * dot(n, n);
  let cp = add(
    p1,
    scale(
      add(
        scale(cross(n, e1), dot(e2, e2)),
        scale(cross(e2, n), dot(e1, e1)),
      ),
      1.0 / denom,
    ),
  );
  let r = norm(sub(p1, cp));
  let normal = unit(n).unwrap_or([0.0, 0.0, 1.0]);
  let dims = input_dim.clamp(2, 3);
  Val::list([Val::vec(cp[..dims].to_vec()), Val::Num(r), Val::vec(normal)])
    .to_lua(lua)
}

fn circle_point_tangents(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let r = match a.radius("r", "d", None) {
    Some(r) => r,
    None => return a.err("give a radius or a diameter"),
  };
  let cp = v2(&a.need_vec("cp")?);
  let pt = v2(&a.need_vec("pt")?);
  let delta = [pt[0] - cp[0], pt[1] - cp[1]];
  let dist = (delta[0] * delta[0] + delta[1] * delta[1]).sqrt();
  let baseang = delta[1].atan2(delta[0]).to_degrees();
  if dist < r - EPS {
    return Val::List(vec![]).to_lua(lua);
  }
  if (dist - r).abs() < EPS {
    return Val::list([Val::vec(pt)]).to_lua(lua);
  }
  let rel = (r / dist).clamp(-1.0, 1.0).acos().to_degrees();
  let pts: Vec<Val> = [baseang + rel, baseang - rel]
    .iter()
    .map(|ang| {
      let (s, c) = ang.to_radians().sin_cos();
      Val::vec([cp[0] + r * c, cp[1] + r * s])
    })
    .collect();
  Val::List(pts).to_lua(lua)
}

fn circle_circle_tangents(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let r1 = match a.radius("r1", "d1", None) {
    Some(r) => r,
    None => return a.err("give r1 or d1"),
  };
  let r2 = match a.radius("r2", "d2", None) {
    Some(r) => r,
    None => return a.err("give r2 or d2"),
  };
  let c1 = v2(&a.need_vec("cp1")?);
  let c2 = v2(&a.need_vec("cp2")?);
  let d = [c2[0] - c1[0], c2[1] - c1[1]];
  let dist = (d[0] * d[0] + d[1] * d[1]).sqrt();
  if dist < EPS {
    return Val::List(vec![]).to_lua(lua);
  }
  let base = d[1].atan2(d[0]);
  let mut out: Vec<Val> = Vec::new();
  // The outer pair keeps both circles on the same side of the line, the
  // inner pair crosses between them.
  for sign in [1.0f64, -1.0] {
    let dr = r1 - sign * r2;
    if dist < dr.abs() - EPS {
      continue;
    }
    let ang = (dr / dist).clamp(-1.0, 1.0).acos();
    for turn in [1.0f64, -1.0] {
      let a1 = base + turn * ang;
      let (s, c) = a1.sin_cos();
      let t1 = [c1[0] + r1 * c, c1[1] + r1 * s];
      let t2 = [c2[0] + sign * r2 * c, c2[1] + sign * r2 * s];
      out.push(Val::list([Val::vec(t1), Val::vec(t2)]));
    }
  }
  Val::List(out).to_lua(lua)
}

fn sphere_line_intersection(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let r = match a.radius("r", "d", None) {
    Some(r) => r,
    None => return a.err("give a radius or a diameter"),
  };
  let cp = a
    .val("cp")
    .and_then(|v| v.as_vec())
    .map(|p| v3(&p))
    .unwrap_or([0.0; 3]);
  let (p1, p2) = line_of(a, "line")?;
  let b = bounds(a, "bounded");
  let eps = a.num_or("eps", EPS);

  let d = sub(p2, p1);
  let f = sub(p1, cp);
  let qa = dot(d, d);
  let qb = 2.0 * dot(f, d);
  let qc = dot(f, f) - r * r;
  let disc = qb * qb - 4.0 * qa * qc;
  if qa < eps || disc < 0.0 {
    return Val::List(vec![]).to_lua(lua);
  }
  let root = disc.sqrt();
  let ts = [(-qb - root) / (2.0 * qa), (-qb + root) / (2.0 * qa)];
  let hits: Vec<Val> = ts
    .iter()
    .filter(|t| (!b[0] || **t >= -eps) && (!b[1] || **t <= 1.0 + eps))
    .map(|t| Val::vec(add(p1, scale(d, *t))))
    .collect();
  Val::List(hits).to_lua(lua)
}

// ---------------------------------------------------------------------------
// Polygons
// ---------------------------------------------------------------------------

fn signed_area2(poly: &[[f64; 2]]) -> f64 {
  let n = poly.len();
  if n < 3 {
    return 0.0;
  }
  let mut sum = 0.0;
  for i in 0..n {
    let p = poly[i];
    let q = poly[(i + 1) % n];
    sum += p[0] * q[1] - q[0] * p[1];
  }
  sum / 2.0
}

fn polygon_area(_lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let poly = a.need_matrix("poly")?;
  let signed = a.bool_or("signed", false);
  if poly.len() < 3 {
    return Ok(LuaValue::Number(0.0));
  }
  let area = if poly[0].len() == 2 {
    signed_area2(&poly.iter().map(|p| v2(p)).collect::<Vec<_>>())
  } else {
    // In 3D the area is the length of the summed cross products, signed
    // against the polygon's own normal.
    let pts: Vec<[f64; 3]> = poly.iter().map(|p| v3(p)).collect();
    let mut acc = [0.0; 3];
    for i in 1..pts.len() - 1 {
      acc = add(acc, cross(sub(pts[i], pts[0]), sub(pts[i + 1], pts[0])));
    }
    let mag = norm(acc) / 2.0;
    match plane_through(&pts) {
      Some(p) if dot([p[0], p[1], p[2]], acc) < 0.0 => -mag,
      _ => mag,
    }
  };
  Ok(LuaValue::Number(if signed { area } else { area.abs() }))
}

fn centroid(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let poly = a.need_matrix("object")?;
  if poly.is_empty() {
    return a.err("the polygon cannot be empty");
  }
  if poly[0].len() == 2 {
    let pts: Vec<[f64; 2]> = poly.iter().map(|p| v2(p)).collect();
    let area = signed_area2(&pts);
    if area.abs() < EPS {
      // A degenerate outline has no area to weight by, so the vertices are
      // averaged instead.
      let n = pts.len() as f64;
      let sum = pts
        .iter()
        .fold([0.0, 0.0], |acc, p| [acc[0] + p[0], acc[1] + p[1]]);
      return num_list(lua, &[sum[0] / n, sum[1] / n]);
    }
    let mut c = [0.0, 0.0];
    for i in 0..pts.len() {
      let p = pts[i];
      let q = pts[(i + 1) % pts.len()];
      let w = p[0] * q[1] - q[0] * p[1];
      c[0] += (p[0] + q[0]) * w;
      c[1] += (p[1] + q[1]) * w;
    }
    return num_list(lua, &[c[0] / (6.0 * area), c[1] / (6.0 * area)]);
  }

  // A 3D polygon is flattened onto its own plane, centred there, and lifted
  // back, so the answer is the area centroid rather than the vertex mean.
  let pts: Vec<[f64; 3]> = poly.iter().map(|p| v3(p)).collect();
  let Some(plane) = plane_through(&pts) else {
    let n = pts.len() as f64;
    let sum = pts.iter().fold([0.0; 3], |acc, p| add(acc, *p));
    return num_list(lua, &scale(sum, 1.0 / n));
  };
  let n = [plane[0], plane[1], plane[2]];
  let origin = pts[0];
  let u = unit(sub(pts[1], pts[0])).unwrap_or([1.0, 0.0, 0.0]);
  let v = cross(n, u);
  let flat: Vec<[f64; 2]> = pts
    .iter()
    .map(|p| {
      let d = sub(*p, origin);
      [dot(d, u), dot(d, v)]
    })
    .collect();
  let area = signed_area2(&flat);
  if area.abs() < EPS {
    let count = pts.len() as f64;
    let sum = pts.iter().fold([0.0; 3], |acc, p| add(acc, *p));
    return num_list(lua, &scale(sum, 1.0 / count));
  }
  let mut c = [0.0, 0.0];
  for i in 0..flat.len() {
    let p = flat[i];
    let q = flat[(i + 1) % flat.len()];
    let w = p[0] * q[1] - q[0] * p[1];
    c[0] += (p[0] + q[0]) * w;
    c[1] += (p[1] + q[1]) * w;
  }
  let cx = c[0] / (6.0 * area);
  let cy = c[1] / (6.0 * area);
  num_list(lua, &add(origin, add(scale(u, cx), scale(v, cy))))
}

fn polygon_normal(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let pts = a.need_points3("poly")?;
  let mut acc = [0.0; 3];
  for i in 1..pts.len().saturating_sub(1) {
    acc = add(acc, cross(sub(pts[i], pts[0]), sub(pts[i + 1], pts[i])));
  }
  // BOSL2 negates, so a counter-clockwise outline seen from +Z gives -Z.
  match unit(scale(acc, -1.0)) {
    Some(n) => num_list(lua, &n),
    None => Ok(LuaValue::Nil),
  }
}

/// Where a point lies relative to a polygon: inside, outside, or on the edge.
fn point_in_polygon(_lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let pt = v2(&a.need_vec("point")?);
  let poly: Vec<[f64; 2]> =
    a.need_matrix("poly")?.iter().map(|p| v2(p)).collect();
  let eps = a.num_or("eps", EPS);
  let nonzero = a.bool_or("nonzero", false);
  let n = poly.len();
  if n < 3 {
    return Ok(LuaValue::Number(-1.0));
  }

  // A point on the boundary counts as neither in nor out.
  for i in 0..n {
    let p = poly[i];
    let q = poly[(i + 1) % n];
    let d = [q[0] - p[0], q[1] - p[1]];
    let len2 = d[0] * d[0] + d[1] * d[1];
    let t = if len2 < eps {
      0.0
    } else {
      (((pt[0] - p[0]) * d[0] + (pt[1] - p[1]) * d[1]) / len2).clamp(0.0, 1.0)
    };
    let closest = [p[0] + d[0] * t, p[1] + d[1] * t];
    let gap =
      ((pt[0] - closest[0]).powi(2) + (pt[1] - closest[1]).powi(2)).sqrt();
    if gap <= eps {
      return Ok(LuaValue::Number(0.0));
    }
  }

  let inside = if nonzero {
    // The winding number counts how many times the outline circles the
    // point, which fills self-overlapping regions.
    let mut winding = 0i32;
    for i in 0..n {
      let p = poly[i];
      let q = poly[(i + 1) % n];
      if p[1] <= pt[1] {
        if q[1] > pt[1]
          && cross2([q[0] - p[0], q[1] - p[1]], [pt[0] - p[0], pt[1] - p[1]])
            > 0.0
        {
          winding += 1;
        }
      } else if q[1] <= pt[1]
        && cross2([q[0] - p[0], q[1] - p[1]], [pt[0] - p[0], pt[1] - p[1]])
          < 0.0
      {
        winding -= 1;
      }
    }
    winding != 0
  } else {
    // The even-odd rule: count the crossings of a ray to the right.
    let mut crossings = 0;
    for i in 0..n {
      let p = poly[i];
      let q = poly[(i + 1) % n];
      if (p[1] > pt[1]) != (q[1] > pt[1]) {
        let x = p[0] + (pt[1] - p[1]) / (q[1] - p[1]) * (q[0] - p[0]);
        if x > pt[0] {
          crossings += 1;
        }
      }
    }
    crossings % 2 == 1
  };
  Ok(LuaValue::Number(if inside { 1.0 } else { -1.0 }))
}

fn polygon_line_intersection(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let poly = a.need_matrix("poly")?;
  let (l1, l2) = line_of(a, "line")?;
  let b = bounds(a, "bounded");
  let eps = a.num_or("eps", EPS);
  if poly.is_empty() {
    return Ok(LuaValue::Nil);
  }

  if poly[0].len() == 2 {
    // In 2D the answer is where the line crosses the outline.
    let pts: Vec<[f64; 2]> = poly.iter().map(|p| v2(p)).collect();
    let mut hits: Vec<Val> = Vec::new();
    for i in 0..pts.len() {
      let p = pts[i];
      let q = pts[(i + 1) % pts.len()];
      if let Some((x, t, u)) = intersect2([l1[0], l1[1]], [l2[0], l2[1]], p, q)
      {
        let in_line = (!b[0] || t >= -eps) && (!b[1] || t <= 1.0 + eps);
        if in_line && u >= -eps && u <= 1.0 + eps {
          hits.push(Val::vec(x));
        }
      }
    }
    return Val::List(hits).to_lua(lua);
  }

  // In 3D the line has to meet the polygon's plane inside the outline.
  let pts: Vec<[f64; 3]> = poly.iter().map(|p| v3(p)).collect();
  let Some(plane) = plane_through(&pts) else {
    return Ok(LuaValue::Nil);
  };
  let n = [plane[0], plane[1], plane[2]];
  let d = sub(l2, l1);
  let denom = dot(n, d);
  if denom.abs() < eps {
    return Ok(LuaValue::Nil);
  }
  let t = (plane[3] - dot(n, l1)) / denom;
  if (b[0] && t < -eps) || (b[1] && t > 1.0 + eps) {
    return Ok(LuaValue::Nil);
  }
  let hit = add(l1, scale(d, t));

  // Flatten onto the plane to decide whether the hit is inside the outline.
  let origin = pts[0];
  let u = unit(sub(pts[1], pts[0])).unwrap_or([1.0, 0.0, 0.0]);
  let v = cross(n, u);
  let flat: Vec<[f64; 2]> = pts
    .iter()
    .map(|p| {
      let dd = sub(*p, origin);
      [dot(dd, u), dot(dd, v)]
    })
    .collect();
  let dd = sub(hit, origin);
  let hit2 = [dot(dd, u), dot(dd, v)];
  let mut crossings = 0;
  for i in 0..flat.len() {
    let p = flat[i];
    let q = flat[(i + 1) % flat.len()];
    if (p[1] > hit2[1]) != (q[1] > hit2[1]) {
      let x = p[0] + (hit2[1] - p[1]) / (q[1] - p[1]) * (q[0] - p[0]);
      if x > hit2[0] {
        crossings += 1;
      }
    }
  }
  if crossings % 2 == 1 {
    num_list(lua, &hit)
  } else {
    Ok(LuaValue::Nil)
  }
}

fn is_polygon_clockwise(_lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let poly: Vec<[f64; 2]> =
    a.need_matrix("poly")?.iter().map(|p| v2(p)).collect();
  Ok(LuaValue::Boolean(signed_area2(&poly) < 0.0))
}

/// Reverse a polygon if it does not already wind the way asked for.
fn wind(lua: &Lua, a: &Args, clockwise: bool) -> LuaResult<LuaValue> {
  let poly = a.need_matrix("poly")?;
  let flat: Vec<[f64; 2]> = poly.iter().map(|p| v2(p)).collect();
  let is_cw = signed_area2(&flat) < 0.0;
  let out: Vec<Vec<f64>> = if is_cw == clockwise {
    poly
  } else {
    poly.into_iter().rev().collect()
  };
  matrix(lua, &out)
}

fn clockwise_polygon(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  wind(lua, a, true)
}

fn ccw_polygon(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  wind(lua, a, false)
}

fn reverse_polygon(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let poly = a.need_matrix("poly")?;
  // Reversing keeps the same starting vertex and flips the direction.
  let mut out = vec![poly[0].clone()];
  out.extend(poly[1..].iter().rev().cloned());
  matrix(lua, &out)
}

fn reindex_polygon(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let reference = a.need_matrix("reference")?;
  let poly = a.need_matrix("poly")?;
  if reference.len() != poly.len() || poly.is_empty() {
    return a.err("both polygons must have the same number of points");
  }
  // Try every rotation and keep the one whose vertices sit closest to the
  // reference, which is what lines two outlines up before lofting them.
  let n = poly.len();
  let cost = |shift: usize| -> f64 {
    (0..n)
      .map(|i| {
        let p = v3(&poly[(i + shift) % n]);
        let q = v3(&reference[i]);
        let d = sub(p, q);
        dot(d, d)
      })
      .sum()
  };
  let best = (0..n)
    .min_by(|x, y| cost(*x).total_cmp(&cost(*y)))
    .unwrap_or(0);
  let out: Vec<Vec<f64>> =
    (0..n).map(|i| poly[(i + best) % n].clone()).collect();
  matrix(lua, &out)
}

fn align_polygon(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let reference = a.need_matrix("reference")?;
  let poly = a.need_matrix("poly")?;
  let angles = a.nums("angles").unwrap_or_default();
  if poly.is_empty() || reference.is_empty() {
    return a.err("both polygons must have points");
  }
  // Spin the polygon through each candidate angle and keep whichever lands
  // closest to the reference.
  let mut best: Option<(f64, Vec<Vec<f64>>)> = None;
  let candidates = if angles.is_empty() {
    (0..360).map(|d| d as f64).collect()
  } else {
    angles
  };
  for ang in candidates {
    let (s, c) = ang.to_radians().sin_cos();
    let turned: Vec<Vec<f64>> = poly
      .iter()
      .map(|p| {
        let q = v2(p);
        vec![q[0] * c - q[1] * s, q[0] * s + q[1] * c]
      })
      .collect();
    let score: f64 = turned
      .iter()
      .map(|p| {
        reference
          .iter()
          .map(|q| {
            let d = [p[0] - q[0], p[1] - q[1]];
            d[0] * d[0] + d[1] * d[1]
          })
          .fold(f64::INFINITY, f64::min)
      })
      .sum();
    if best.as_ref().is_none_or(|(b, _)| score < *b) {
      best = Some((score, turned));
    }
  }
  match best {
    Some((_, p)) => matrix(lua, &p),
    None => a.err("no candidate alignment was found"),
  }
}

fn are_polygons_equal(_lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let p1 = a.need_matrix("poly1")?;
  let p2 = a.need_matrix("poly2")?;
  let eps = a.num_or("eps", EPS);
  if p1.len() != p2.len() {
    return Ok(LuaValue::Boolean(false));
  }
  let n = p1.len();
  if n == 0 {
    return Ok(LuaValue::Boolean(true));
  }
  // The same outline may start at a different vertex, so every rotation is
  // a candidate.
  let same = (0..n).any(|shift| {
    (0..n).all(|i| {
      let x = v3(&p1[i]);
      let y = v3(&p2[(i + shift) % n]);
      norm(sub(x, y)) <= eps
    })
  });
  Ok(LuaValue::Boolean(same))
}

fn is_polygon_convex(_lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let poly = a.need_matrix("poly")?;
  let eps = a.num_or("eps", EPS);
  let n = poly.len();
  if n < 3 {
    return Ok(LuaValue::Boolean(false));
  }
  if poly[0].len() == 2 {
    // Every turn must go the same way round.
    let pts: Vec<[f64; 2]> = poly.iter().map(|p| v2(p)).collect();
    let mut sign = 0.0f64;
    for i in 0..n {
      let p = pts[i];
      let q = pts[(i + 1) % n];
      let r = pts[(i + 2) % n];
      let z = cross2([q[0] - p[0], q[1] - p[1]], [r[0] - q[0], r[1] - q[1]]);
      if z.abs() > eps {
        if sign == 0.0 {
          sign = z.signum();
        } else if z.signum() != sign {
          return Ok(LuaValue::Boolean(false));
        }
      }
    }
    return Ok(LuaValue::Boolean(true));
  }
  let pts: Vec<[f64; 3]> = poly.iter().map(|p| v3(p)).collect();
  let Some(plane) = plane_through(&pts) else {
    return Ok(LuaValue::Boolean(false));
  };
  let nrm = [plane[0], plane[1], plane[2]];
  let mut sign = 0.0f64;
  for i in 0..n {
    let p = pts[i];
    let q = pts[(i + 1) % n];
    let r = pts[(i + 2) % n];
    let z = dot(nrm, cross(sub(q, p), sub(r, q)));
    if z.abs() > eps {
      if sign == 0.0 {
        sign = z.signum();
      } else if z.signum() != sign {
        return Ok(LuaValue::Boolean(false));
      }
    }
  }
  Ok(LuaValue::Boolean(true))
}

/// Split a simple polygon into triangles, by clipping ears.
fn triangulate(poly: &[[f64; 2]]) -> Option<Vec<[usize; 3]>> {
  let n = poly.len();
  if n < 3 {
    return Some(vec![]);
  }
  let ccw = signed_area2(poly) > 0.0;
  let mut remaining: Vec<usize> = if ccw {
    (0..n).collect()
  } else {
    (0..n).rev().collect()
  };
  let mut out = Vec::with_capacity(n - 2);

  let inside = |a: [f64; 2], b: [f64; 2], c: [f64; 2], p: [f64; 2]| {
    let d1 = cross2([b[0] - a[0], b[1] - a[1]], [p[0] - a[0], p[1] - a[1]]);
    let d2 = cross2([c[0] - b[0], c[1] - b[1]], [p[0] - b[0], p[1] - b[1]]);
    let d3 = cross2([a[0] - c[0], a[1] - c[1]], [p[0] - c[0], p[1] - c[1]]);
    d1 >= -EPS && d2 >= -EPS && d3 >= -EPS
  };

  let mut guard = 0;
  while remaining.len() > 3 {
    guard += 1;
    if guard > n * n + 16 {
      // A self-intersecting outline has no ear to clip; give up rather than
      // spin, and let the caller decide what to do.
      return None;
    }
    let m = remaining.len();
    let mut clipped = false;
    for k in 0..m {
      let (i0, i1, i2) = (
        remaining[(k + m - 1) % m],
        remaining[k],
        remaining[(k + 1) % m],
      );
      let (a, b, c) = (poly[i0], poly[i1], poly[i2]);
      // A convex corner with no other vertex inside it is an ear.
      if cross2([b[0] - a[0], b[1] - a[1]], [c[0] - b[0], c[1] - b[1]]) <= EPS {
        continue;
      }
      let blocked = remaining
        .iter()
        .any(|j| *j != i0 && *j != i1 && *j != i2 && inside(a, b, c, poly[*j]));
      if !blocked {
        out.push([i0, i1, i2]);
        remaining.remove(k);
        clipped = true;
        break;
      }
    }
    if !clipped {
      return None;
    }
  }
  if remaining.len() == 3 {
    out.push([remaining[0], remaining[1], remaining[2]]);
  }
  Some(out)
}

fn polygon_triangulate(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let poly = a.need_matrix("poly")?;
  if poly.is_empty() {
    return Val::List(vec![]).to_lua(lua);
  }
  // A 3D polygon is triangulated on its own plane and the indices reused.
  let flat: Vec<[f64; 2]> = if poly[0].len() == 2 {
    poly.iter().map(|p| v2(p)).collect()
  } else {
    let pts: Vec<[f64; 3]> = poly.iter().map(|p| v3(p)).collect();
    let Some(plane) = plane_through(&pts) else {
      return a.err("the polygon is degenerate");
    };
    let n = [plane[0], plane[1], plane[2]];
    let origin = pts[0];
    let u = unit(sub(pts[1], pts[0])).unwrap_or([1.0, 0.0, 0.0]);
    let v = cross(n, u);
    pts
      .iter()
      .map(|p| {
        let d = sub(*p, origin);
        [dot(d, u), dot(d, v)]
      })
      .collect()
  };
  match triangulate(&flat) {
    Some(tris) => {
      Val::list(tris.iter().map(|t| Val::vec(t.iter().map(|i| *i as f64))))
        .to_lua(lua)
    }
    None => {
      a.err("the polygon could not be triangulated; is it self-intersecting?")
    }
  }
}

// ---------------------------------------------------------------------------
// Hulls
// ---------------------------------------------------------------------------

/// The convex hull of 2D points, as indices in counter-clockwise order.
fn hull2d(points: &[[f64; 2]]) -> Vec<usize> {
  let n = points.len();
  if n < 3 {
    return (0..n).collect();
  }
  let mut order: Vec<usize> = (0..n).collect();
  order.sort_by(|i, j| {
    points[*i][0]
      .total_cmp(&points[*j][0])
      .then(points[*i][1].total_cmp(&points[*j][1]))
  });

  // Andrew's monotone chain: sweep once for the lower boundary and once for
  // the upper, discarding any point that would make a right turn.
  let turn = |o: usize, a: usize, b: usize| {
    cross2(
      [points[a][0] - points[o][0], points[a][1] - points[o][1]],
      [points[b][0] - points[o][0], points[b][1] - points[o][1]],
    )
  };
  let mut hull: Vec<usize> = Vec::with_capacity(2 * n);
  for &i in &order {
    while hull.len() >= 2
      && turn(hull[hull.len() - 2], hull[hull.len() - 1], i) <= EPS
    {
      hull.pop();
    }
    hull.push(i);
  }
  let lower = hull.len() + 1;
  for &i in order.iter().rev().skip(1) {
    while hull.len() >= lower
      && turn(hull[hull.len() - 2], hull[hull.len() - 1], i) <= EPS
    {
      hull.pop();
    }
    hull.push(i);
  }
  hull.pop();
  hull
}

fn hull2d_path(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let pts: Vec<[f64; 2]> =
    a.need_matrix("points")?.iter().map(|p| v2(p)).collect();
  num_list(
    lua,
    &hull2d(&pts).iter().map(|i| *i as f64).collect::<Vec<_>>(),
  )
}

/// The faces of the convex hull of 3D points, built incrementally.
pub(crate) fn hull3d(points: &[[f64; 3]]) -> Option<Vec<[usize; 3]>> {
  let n = points.len();
  if n < 4 {
    return None;
  }
  // Start from a tetrahedron of four points that are not coplanar.
  let mut base = None;
  'outer: for i in 0..n {
    for j in (i + 1)..n {
      for k in (j + 1)..n {
        let nrm = cross(sub(points[j], points[i]), sub(points[k], points[i]));
        if norm(nrm) < EPS {
          continue;
        }
        for l in (k + 1)..n {
          if dot(nrm, sub(points[l], points[i])).abs() > EPS {
            base = Some([i, j, k, l]);
            break 'outer;
          }
        }
      }
    }
  }
  let [i, j, k, l] = base?;

  let mut faces: Vec<[usize; 3]> =
    vec![[i, j, k], [i, k, l], [i, l, j], [j, l, k]];
  // Orient every starting face outward from the tetrahedron's centre.
  let centre = scale(
    add(add(points[i], points[j]), add(points[k], points[l])),
    0.25,
  );
  for f in &mut faces {
    let nrm = cross(
      sub(points[f[1]], points[f[0]]),
      sub(points[f[2]], points[f[0]]),
    );
    if dot(nrm, sub(centre, points[f[0]])) > 0.0 {
      f.swap(1, 2);
    }
  }

  for (p, point) in points.iter().enumerate() {
    if [i, j, k, l].contains(&p) {
      continue;
    }
    // Drop every face the new point can see, then close the hole it leaves
    // by joining the point to the horizon edges.
    let mut visible = Vec::new();
    for (fi, f) in faces.iter().enumerate() {
      let nrm = cross(
        sub(points[f[1]], points[f[0]]),
        sub(points[f[2]], points[f[0]]),
      );
      if dot(nrm, sub(*point, points[f[0]])) > EPS {
        visible.push(fi);
      }
    }
    if visible.is_empty() {
      continue;
    }
    let mut edges: Vec<(usize, usize)> = Vec::new();
    for fi in &visible {
      let f = faces[*fi];
      for e in [(f[0], f[1]), (f[1], f[2]), (f[2], f[0])] {
        // An edge shared by two visible faces is interior, not horizon.
        if let Some(pos) =
          edges.iter().position(|x| *x == (e.1, e.0) || *x == e)
        {
          edges.remove(pos);
        } else {
          edges.push(e);
        }
      }
    }
    for fi in visible.iter().rev() {
      faces.remove(*fi);
    }
    for (u, v) in edges {
      faces.push([u, v, p]);
    }
  }
  Some(faces)
}

fn hull3d_faces(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let pts = a.need_points3("points")?;
  match hull3d(&pts) {
    Some(faces) => {
      Val::list(faces.iter().map(|f| Val::vec(f.iter().map(|i| *i as f64))))
        .to_lua(lua)
    }
    None => {
      // Fewer than four points, or all of them on one plane: the hull is
      // flat, so the 2D hull of the points describes it.
      Val::List(vec![]).to_lua(lua)
    }
  }
}

// ---------------------------------------------------------------------------
// Convex bodies
// ---------------------------------------------------------------------------

/// The point of a convex hull furthest along a direction.
fn support(points: &[[f64; 3]], dir: [f64; 3]) -> [f64; 3] {
  *points
    .iter()
    .max_by(|a, b| dot(**a, dir).total_cmp(&dot(**b, dir)))
    .unwrap_or(&[0.0; 3])
}

/// The point on a segment closest to the origin.
fn closest_on_segment(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
  let ab = sub(b, a);
  let len2 = dot(ab, ab);
  if len2 < EPS {
    return a;
  }
  let t = (-dot(a, ab) / len2).clamp(0.0, 1.0);
  add(a, scale(ab, t))
}

/// The point on a triangle closest to the origin.
///
/// The origin is placed against each edge and vertex region in turn, which
/// is what makes the answer exact rather than merely nearby.
fn closest_on_triangle(a: [f64; 3], b: [f64; 3], c: [f64; 3]) -> [f64; 3] {
  let ab = sub(b, a);
  let ac = sub(c, a);
  let ao = scale(a, -1.0);

  let d1 = dot(ab, ao);
  let d2 = dot(ac, ao);
  if d1 <= 0.0 && d2 <= 0.0 {
    return a;
  }
  let bo = scale(b, -1.0);
  let d3 = dot(ab, bo);
  let d4 = dot(ac, bo);
  if d3 >= 0.0 && d4 <= d3 {
    return b;
  }
  let vc = d1 * d4 - d3 * d2;
  if vc <= 0.0 && d1 >= 0.0 && d3 <= 0.0 {
    return add(a, scale(ab, d1 / (d1 - d3)));
  }
  let co = scale(c, -1.0);
  let d5 = dot(ab, co);
  let d6 = dot(ac, co);
  if d6 >= 0.0 && d5 <= d6 {
    return c;
  }
  let vb = d5 * d2 - d1 * d6;
  if vb <= 0.0 && d2 >= 0.0 && d6 <= 0.0 {
    return add(a, scale(ac, d2 / (d2 - d6)));
  }
  let va = d3 * d6 - d5 * d4;
  if va <= 0.0 && (d4 - d3) >= 0.0 && (d5 - d6) >= 0.0 {
    let w = (d4 - d3) / ((d4 - d3) + (d5 - d6));
    return add(b, scale(sub(c, b), w));
  }
  // Inside the face: interpolate by the barycentric weights.
  let denom = 1.0 / (va + vb + vc);
  add(a, add(scale(ab, vb * denom), scale(ac, vc * denom)))
}

/// The point on a simplex of up to four points closest to the origin.
fn closest_on_simplex(pts: &[[f64; 3]]) -> [f64; 3] {
  match pts.len() {
    0 => [0.0; 3],
    1 => pts[0],
    2 => closest_on_segment(pts[0], pts[1]),
    3 => closest_on_triangle(pts[0], pts[1], pts[2]),
    _ => {
      // A tetrahedron either contains the origin or has it outside one of
      // its faces; the nearest of the four faces gives the answer.
      let faces = [
        [pts[0], pts[1], pts[2]],
        [pts[0], pts[1], pts[3]],
        [pts[0], pts[2], pts[3]],
        [pts[1], pts[2], pts[3]],
      ];
      let mut best = closest_on_triangle(faces[0][0], faces[0][1], faces[0][2]);
      for f in &faces[1..] {
        let p = closest_on_triangle(f[0], f[1], f[2]);
        if norm(p) < norm(best) {
          best = p;
        }
      }
      best
    }
  }
}

/// The gap between two convex point sets, zero when they touch or overlap.
///
/// This walks the Minkowski difference towards the origin, which is the
/// standard way to answer both "how far apart" and "do they collide" without
/// building either hull.
fn gjk_distance(a: &[[f64; 3]], b: &[[f64; 3]]) -> f64 {
  if a.is_empty() || b.is_empty() {
    return f64::INFINITY;
  }
  let minkowski = |d: [f64; 3]| sub(support(a, d), support(b, scale(d, -1.0)));

  let mut simplex: Vec<[f64; 3]> = vec![minkowski([1.0, 0.0, 0.0])];
  for _ in 0..128 {
    let closest = closest_on_simplex(&simplex);
    let d = norm(closest);
    if d < 1e-12 {
      // The origin is inside the Minkowski difference, so the sets overlap.
      return 0.0;
    }
    let dir = scale(closest, -1.0 / d);
    let p = minkowski(dir);
    // Stop once no support point lies further towards the origin: the
    // current distance is then the true one.
    if dot(p, dir) - dot(closest, dir) < 1e-12 {
      return d;
    }
    simplex.push(p);
    // Keep only the points that carry the closest feature, so the simplex
    // never grows past four.
    if simplex.len() > 4 {
      let target = closest_on_simplex(&simplex);
      simplex.sort_by(|x, y| {
        norm(sub(*x, target)).total_cmp(&norm(sub(*y, target)))
      });
      simplex.truncate(4);
    }
  }
  norm(closest_on_simplex(&simplex))
}

fn convex_distance(_lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let p1 = a.need_points3("points1")?;
  let p2 = a.need_points3("points2")?;
  Ok(LuaValue::Number(gjk_distance(&p1, &p2)))
}

fn convex_collision(_lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let p1 = a.need_points3("points1")?;
  let p2 = a.need_points3("points2")?;
  let eps = a.num_or("eps", EPS);
  Ok(LuaValue::Boolean(gjk_distance(&p1, &p2) <= eps))
}

/// Take a rotation matrix apart into an axis, an angle and a translation.
fn rot_decode(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let m = a.need_matrix("M")?;
  if m.len() != 4 || m.iter().any(|r| r.len() != 4) {
    return a.err("M must be a 4×4 transformation matrix");
  }
  let r = [
    [m[0][0], m[0][1], m[0][2]],
    [m[1][0], m[1][1], m[1][2]],
    [m[2][0], m[2][1], m[2][2]],
  ];
  let translation = [m[0][3], m[1][3], m[2][3]];
  let trace = r[0][0] + r[1][1] + r[2][2];
  let angle = ((trace - 1.0) / 2.0).clamp(-1.0, 1.0).acos();

  // The axis is the eigenvector with eigenvalue 1. Near half a turn the
  // usual off-diagonal formula cancels, so the axis is read off the diagonal
  // of `R + I` instead.
  let axis = if (angle - std::f64::consts::PI).abs() < 1e-6 {
    let d = [
      ((r[0][0] + 1.0) / 2.0).max(0.0).sqrt(),
      ((r[1][1] + 1.0) / 2.0).max(0.0).sqrt(),
      ((r[2][2] + 1.0) / 2.0).max(0.0).sqrt(),
    ];
    let largest = (0..3).max_by(|i, j| d[*i].total_cmp(&d[*j])).unwrap_or(0);
    let mut v = d;
    // Fix the signs from the off-diagonal terms relative to the largest.
    for i in 0..3 {
      if i != largest {
        let off = r[largest][i] + r[i][largest];
        if off < 0.0 {
          v[i] = -v[i];
        }
      }
    }
    unit(v).unwrap_or([0.0, 0.0, 1.0])
  } else if angle.abs() < 1e-12 {
    [0.0, 0.0, 1.0]
  } else {
    unit([r[2][1] - r[1][2], r[0][2] - r[2][0], r[1][0] - r[0][1]])
      .unwrap_or([0.0, 0.0, 1.0])
  };

  // Split the translation into the part along the axis and the centre of
  // rotation that accounts for the rest.
  let along = dot(translation, axis);
  let perp = sub(translation, scale(axis, along));
  let centre = if angle.abs() < 1e-12 {
    [0.0; 3]
  } else {
    let half = angle / 2.0;
    let k = 0.5;
    let rotated = cross(axis, perp);
    add(scale(perp, k), scale(rotated, k / half.tan()))
  };

  Val::list([
    Val::Num(angle.to_degrees()),
    Val::vec(axis),
    Val::vec(centre),
    Val::Num(along),
  ])
  .to_lua(lua)
}

pub fn register(lua: &Lua, bosl: &mlua::Table) -> LuaResult<()> {
  register_all(
    lua,
    bosl,
    &[
      (
        "is_point_on_line",
        &["point", "line", "bounded", "eps"],
        is_point_on_line as PureFn,
      ),
      ("is_collinear", &["p1", "p2", "p3", "eps"], is_collinear),
      (
        "point_line_distance",
        &["pt", "line", "bounded"],
        point_line_distance,
      ),
      (
        "segment_distance",
        &["seg1", "seg2", "eps"],
        segment_distance,
      ),
      ("line_normal", &["p1", "p2"], line_normal),
      (
        "line_intersection",
        &["line1", "line2", "bounded1", "bounded2", "bounded", "eps"],
        line_intersection,
      ),
      (
        "line_closest_point",
        &["line", "pt", "bounded"],
        line_closest_point,
      ),
      (
        "line_from_points",
        &["points", "check_collinear", "eps"],
        line_from_points,
      ),
      ("is_coplanar", &["points", "eps"], is_coplanar),
      ("plane3pt", &["p1", "p2", "p3"], plane3pt),
      (
        "plane3pt_indexed",
        &["points", "i1", "i2", "i3"],
        plane3pt_indexed,
      ),
      ("plane_from_normal", &["normal", "pt"], plane_from_normal),
      (
        "plane_from_points",
        &["points", "check_coplanar", "eps"],
        plane_from_points,
      ),
      (
        "plane_from_polygon",
        &["poly", "check_coplanar", "eps"],
        plane_from_polygon,
      ),
      ("plane_normal", &["plane"], plane_normal),
      ("plane_offset", &["plane"], plane_offset),
      (
        "plane_line_intersection",
        &["plane", "line", "bounded", "eps"],
        plane_line_intersection,
      ),
      (
        "plane_intersection",
        &["plane1", "plane2", "plane3"],
        plane_intersection,
      ),
      ("plane_line_angle", &["plane", "line"], plane_line_angle),
      (
        "plane_closest_point",
        &["plane", "points"],
        plane_closest_point,
      ),
      (
        "point_plane_distance",
        &["plane", "point"],
        point_plane_distance,
      ),
      (
        "are_points_on_plane",
        &["points", "plane", "eps"],
        are_points_on_plane,
      ),
      (
        "circle_line_intersection",
        &["r", "cp", "line", "bounded", "d", "eps"],
        circle_line_intersection,
      ),
      (
        "circle_circle_intersection",
        &["r1", "cp1", "r2", "cp2", "eps", "d1", "d2"],
        circle_circle_intersection,
      ),
      (
        "circle_2tangents",
        &["r", "pt1", "pt2", "pt3", "tangents", "d"],
        circle_2tangents,
      ),
      ("circle_3points", &["pt1", "pt2", "pt3"], circle_3points),
      (
        "circle_point_tangents",
        &["r", "cp", "pt", "d"],
        circle_point_tangents,
      ),
      (
        "circle_circle_tangents",
        &["r1", "cp1", "r2", "cp2", "d1", "d2"],
        circle_circle_tangents,
      ),
      (
        "sphere_line_intersection",
        &["r", "cp", "line", "bounded", "d", "eps"],
        sphere_line_intersection,
      ),
      ("polygon_area", &["poly", "signed"], polygon_area),
      ("centroid", &["object", "eps"], centroid),
      ("polygon_normal", &["poly"], polygon_normal),
      (
        "point_in_polygon",
        &["point", "poly", "nonzero", "eps"],
        point_in_polygon,
      ),
      (
        "polygon_line_intersection",
        &["poly", "line", "bounded", "nonzero", "eps"],
        polygon_line_intersection,
      ),
      (
        "polygon_triangulate",
        &["poly", "ind", "error", "eps"],
        polygon_triangulate,
      ),
      ("is_polygon_clockwise", &["poly"], is_polygon_clockwise),
      ("clockwise_polygon", &["poly"], clockwise_polygon),
      ("ccw_polygon", &["poly"], ccw_polygon),
      ("reverse_polygon", &["poly"], reverse_polygon),
      (
        "reindex_polygon",
        &["reference", "poly", "return_error"],
        reindex_polygon,
      ),
      (
        "align_polygon",
        &["reference", "poly", "angles", "trans"],
        align_polygon,
      ),
      (
        "are_polygons_equal",
        &["poly1", "poly2", "eps"],
        are_polygons_equal,
      ),
      ("hull2d_path", &["points", "all"], hull2d_path),
      ("hull3d_faces", &["points"], hull3d_faces),
      ("is_polygon_convex", &["poly", "eps"], is_polygon_convex),
      (
        "convex_distance",
        &["points1", "points2", "eps"],
        convex_distance,
      ),
      (
        "convex_collision",
        &["points1", "points2", "eps"],
        convex_collision,
      ),
      ("rot_decode", &["M", "long"], rot_decode),
    ],
  )
}

#[cfg(test)]
mod tests {
  use crate::bosl::register_bosl;
  use mlua::Lua;

  fn eval<T: mlua::FromLua>(code: &str) -> T {
    let lua = Lua::new();
    register_bosl(&lua).unwrap();
    lua
      .load(code)
      .eval()
      .unwrap_or_else(|e| panic!("evaluating {code}: {e}"))
  }

  fn close(a: &[f64], b: &[f64]) -> bool {
    a.len() == b.len() && a.iter().zip(b).all(|(x, y)| (x - y).abs() < 1e-6)
  }

  #[test]
  fn distances_to_lines_and_segments() {
    let d: f64 = eval("return bosl.point_line_distance({0,5}, {{0,0},{10,0}})");
    assert_eq!(d, 5.0);
    // Bounding the line turns it into a segment, so the far end is measured.
    let d: f64 =
      eval("return bosl.point_line_distance({20,0}, {{0,0},{10,0}}, true)");
    assert_eq!(d, 10.0);
  }

  #[test]
  fn line_intersections_respect_their_bounds() {
    let p: Vec<f64> =
      eval("return bosl.line_intersection({{0,0},{10,0}}, {{5,-5},{5,5}})");
    assert!(close(&p, &[5.0, 0.0]), "{p:?}");
    // Two segments that would only meet if extended do not intersect.
    let nil: Option<Vec<f64>> = eval(
      "return bosl.line_intersection({{0,0},{1,0}}, {{5,-5},{5,5}},
        bosl.SEGMENT, bosl.SEGMENT)",
    );
    assert!(nil.is_none());
  }

  #[test]
  fn collinear_points_are_recognised() {
    assert!(eval::<bool>("return bosl.is_collinear({0,0},{1,1},{2,2})"));
    assert!(!eval::<bool>("return bosl.is_collinear({0,0},{1,1},{2,3})"));
    assert!(eval::<bool>(
      "return bosl.is_point_on_line({1,1}, {{0,0},{5,5}})"
    ));
  }

  #[test]
  fn a_plane_through_three_points_has_a_unit_normal() {
    let p: Vec<f64> = eval("return bosl.plane3pt({0,0,5},{1,0,5},{0,1,5})");
    assert!((p[0].abs() + p[1].abs()) < 1e-9, "{p:?}");
    assert!((p[2].abs() - 1.0).abs() < 1e-9, "{p:?}");
    assert!((p[3].abs() - 5.0).abs() < 1e-9, "{p:?}");
    assert_eq!(eval::<f64>("return bosl.plane_offset({0,0,1,5})"), 5.0);
    let n: Vec<f64> = eval("return bosl.plane_normal({0,0,2,10})");
    assert!(close(&n, &[0.0, 0.0, 1.0]), "{n:?}");
  }

  #[test]
  fn distance_to_a_plane_is_signed_by_which_side() {
    let d: f64 = eval("return bosl.point_plane_distance({0,0,1,0}, {1,2,5})");
    assert_eq!(d, 5.0);
    let d: f64 = eval("return bosl.point_plane_distance({0,0,1,0}, {1,2,-5})");
    assert_eq!(d, -5.0);
  }

  #[test]
  fn a_line_meets_a_plane_where_expected() {
    let p: Vec<f64> = eval(
      "return bosl.plane_line_intersection({0,0,1,0}, {{0,0,5},{0,0,-5}})",
    );
    assert!(close(&p, &[0.0, 0.0, 0.0]), "{p:?}");
    // A line parallel to the plane never reaches it.
    let nil: Option<Vec<f64>> =
      eval("return bosl.plane_line_intersection({0,0,1,0}, {{0,0,5},{1,0,5}})");
    assert!(nil.is_none());
  }

  #[test]
  fn two_planes_meet_along_a_line() {
    let l: Vec<Vec<f64>> =
      eval("return bosl.plane_intersection({0,0,1,0}, {0,1,0,0})");
    // The line is the X axis: both points have y = z = 0.
    for p in &l {
      assert!(p[1].abs() < 1e-9 && p[2].abs() < 1e-9, "{l:?}");
    }
  }

  #[test]
  fn circles_intersect_lines_and_each_other() {
    let hits: Vec<Vec<f64>> =
      eval("return bosl.circle_line_intersection(5, {0,0}, {{-10,0},{10,0}})");
    assert_eq!(hits.len(), 2);
    assert!(close(&hits[0], &[-5.0, 0.0]) || close(&hits[0], &[5.0, 0.0]));

    let hits: Vec<Vec<f64>> =
      eval("return bosl.circle_circle_intersection(5, {0,0}, 5, {8,0})");
    assert_eq!(hits.len(), 2);
    assert!((hits[0][0] - 4.0).abs() < 1e-9, "{hits:?}");
  }

  #[test]
  fn a_circle_through_three_points_is_centred_between_them() {
    let cp: Vec<f64> =
      eval("return bosl.circle_3points({1,0},{0,1},{-1,0})[1]");
    assert!(close(&cp, &[0.0, 0.0]), "{cp:?}");
    let r: f64 = eval("return bosl.circle_3points({1,0},{0,1},{-1,0})[2]");
    assert!((r - 1.0).abs() < 1e-9, "{r}");
  }

  #[test]
  fn tangent_points_touch_the_circle() {
    let ts: Vec<Vec<f64>> =
      eval("return bosl.circle_point_tangents(3, {0,0}, {5,0})");
    assert_eq!(ts.len(), 2);
    for t in &ts {
      assert!(((t[0] * t[0] + t[1] * t[1]).sqrt() - 3.0).abs() < 1e-9);
    }
  }

  #[test]
  fn a_sphere_meets_a_line_at_two_points() {
    let hits: Vec<Vec<f64>> = eval(
      "return bosl.sphere_line_intersection(5, {0,0,0}, {{0,0,-10},{0,0,10}})",
    );
    assert_eq!(hits.len(), 2);
  }

  #[test]
  fn polygon_area_and_centroid_of_a_square() {
    let area: f64 =
      eval("return bosl.polygon_area({{0,0},{10,0},{10,10},{0,10}})");
    assert_eq!(area, 100.0);
    // The signed area is negative when the outline runs clockwise.
    let signed: f64 =
      eval("return bosl.polygon_area({{0,0},{0,10},{10,10},{10,0}}, true)");
    assert_eq!(signed, -100.0);
    let c: Vec<f64> =
      eval("return bosl.centroid({{0,0},{10,0},{10,10},{0,10}})");
    assert!(close(&c, &[5.0, 5.0]), "{c:?}");
  }

  #[test]
  fn a_triangles_centroid_is_the_average_of_its_corners() {
    let c: Vec<f64> = eval("return bosl.centroid({{0,0},{9,0},{0,9}})");
    assert!(close(&c, &[3.0, 3.0]), "{c:?}");
  }

  #[test]
  fn point_in_polygon_tells_inside_from_outside_and_the_edge() {
    let square = "{{0,0},{10,0},{10,10},{0,10}}";
    assert_eq!(
      eval::<f64>(&format!("return bosl.point_in_polygon({{5,5}}, {square})")),
      1.0
    );
    assert_eq!(
      eval::<f64>(&format!("return bosl.point_in_polygon({{15,5}}, {square})")),
      -1.0
    );
    assert_eq!(
      eval::<f64>(&format!("return bosl.point_in_polygon({{0,5}}, {square})")),
      0.0
    );
  }

  #[test]
  fn winding_direction_is_reported_and_corrected() {
    assert!(eval::<bool>(
      "return bosl.is_polygon_clockwise({{0,0},{0,10},{10,10},{10,0}})"
    ));
    assert!(!eval::<bool>(
      "return bosl.is_polygon_clockwise({{0,0},{10,0},{10,10},{0,10}})"
    ));
    let ccw: Vec<Vec<f64>> =
      eval("return bosl.ccw_polygon({{0,0},{0,10},{10,10},{10,0}})");
    assert!(!eval::<bool>(&format!(
      "return bosl.is_polygon_clockwise({{{{{},{}}},{{{},{}}},{{{},{}}},{{{},{}}}}})",
      ccw[0][0],
      ccw[0][1],
      ccw[1][0],
      ccw[1][1],
      ccw[2][0],
      ccw[2][1],
      ccw[3][0],
      ccw[3][1]
    )));
  }

  #[test]
  fn convexity_is_detected_in_two_and_three_dimensions() {
    assert!(eval::<bool>(
      "return bosl.is_polygon_convex({{0,0},{10,0},{10,10},{0,10}})"
    ));
    // An arrowhead has one reflex corner.
    assert!(!eval::<bool>(
      "return bosl.is_polygon_convex({{0,0},{10,0},{5,3},{10,10},{0,10}})"
    ));
  }

  #[test]
  fn triangulating_a_polygon_covers_its_whole_area() {
    let total: f64 = eval(
      "local poly = {{0,0},{10,0},{10,10},{5,3},{0,10}}
       local tris = bosl.polygon_triangulate(poly)
       local total = 0
       for _, t in ipairs(tris) do
         local a, b, c = poly[t[1]+1], poly[t[2]+1], poly[t[3]+1]
         total = total + math.abs((b[1]-a[1])*(c[2]-a[2])
                                - (c[1]-a[1])*(b[2]-a[2])) / 2
       end
       return total",
    );
    let area: f64 =
      eval("return bosl.polygon_area({{0,0},{10,0},{10,10},{5,3},{0,10}})");
    assert!((total - area).abs() < 1e-6, "{total} vs {area}");
  }

  #[test]
  fn triangulation_produces_two_fewer_triangles_than_vertices() {
    let n: usize =
      eval("return #bosl.polygon_triangulate({{0,0},{10,0},{10,10},{0,10}})");
    assert_eq!(n, 2);
  }

  #[test]
  fn the_2d_hull_drops_the_interior_points() {
    let hull: Vec<f64> =
      eval("return bosl.hull2d_path({{0,0},{10,0},{10,10},{0,10},{5,5}})");
    assert_eq!(hull.len(), 4, "{hull:?}");
    assert!(
      !hull.contains(&4.0),
      "the interior point is not on the hull"
    );
  }

  #[test]
  fn the_3d_hull_of_a_cube_has_twelve_triangles() {
    let faces: Vec<Vec<f64>> = eval(
      "return bosl.hull3d_faces({
         {0,0,0},{1,0,0},{1,1,0},{0,1,0},
         {0,0,1},{1,0,1},{1,1,1},{0,1,1},
         {0.5,0.5,0.5}})",
    );
    assert_eq!(faces.len(), 12, "{faces:?}");
  }

  #[test]
  fn convex_bodies_report_their_separation_and_collisions() {
    let d: f64 = eval(
      "return bosl.convex_distance({{0,0,0},{1,0,0},{0,1,0},{0,0,1}},
                                   {{5,0,0},{6,0,0},{5,1,0},{5,0,1}})",
    );
    assert!((d - 4.0).abs() < 1e-6, "{d}");
    assert!(!eval::<bool>(
      "return bosl.convex_collision({{0,0,0},{1,0,0},{0,1,0},{0,0,1}},
                                    {{5,0,0},{6,0,0},{5,1,0},{5,0,1}})"
    ));
    // Two boxes sharing a corner region do collide.
    assert!(eval::<bool>(
      "local function box(o, s)
         local out = {}
         for i = 0, 7 do
           out[#out+1] = {o[1] + (i % 2) * s,
                          o[2] + (math.floor(i / 2) % 2) * s,
                          o[3] + (math.floor(i / 4) % 2) * s}
         end
         return out
       end
       return bosl.convex_collision(box({0,0,0}, 2), box({1,1,1}, 2))"
    ));
  }

  #[test]
  fn coplanar_points_are_recognised() {
    assert!(eval::<bool>(
      "return bosl.is_coplanar({{0,0,0},{1,0,0},{0,1,0},{1,1,0}})"
    ));
    assert!(!eval::<bool>(
      "return bosl.is_coplanar({{0,0,0},{1,0,0},{0,1,0},{0,0,1}})"
    ));
  }

  #[test]
  fn rot_decode_recovers_the_angle_and_axis_it_was_given() {
    let turn = "{{0,-1,0,0},{1,0,0,0},{0,0,1,0},{0,0,0,1}}";
    let angle: f64 = eval(&format!("return bosl.rot_decode({turn})[1]"));
    assert!((angle - 90.0).abs() < 1e-6, "{angle}");
    let axis: Vec<f64> = eval(&format!("return bosl.rot_decode({turn})[2]"));
    assert!(close(&axis, &[0.0, 0.0, 1.0]), "{axis:?}");
  }

  #[test]
  fn polygons_that_differ_only_in_starting_vertex_are_equal() {
    assert!(eval::<bool>(
      "return bosl.are_polygons_equal({{0,0},{1,0},{1,1}},
                                      {{1,0},{1,1},{0,0}})"
    ));
    assert!(!eval::<bool>(
      "return bosl.are_polygons_equal({{0,0},{1,0},{1,1}},
                                      {{0,0},{2,0},{1,1}})"
    ));
  }

  #[test]
  fn reindexing_lines_a_polygon_up_with_a_reference() {
    let p: Vec<Vec<f64>> = eval(
      "return bosl.reindex_polygon({{0,0},{10,0},{10,10},{0,10}},
                                   {{10,10},{0,10},{0,0},{10,0}})",
    );
    assert!(close(&p[0], &[0.0, 0.0]), "{p:?}");
  }
}
