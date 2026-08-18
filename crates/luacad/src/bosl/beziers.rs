//! BOSL2's `beziers.scad`: Bézier curves, paths and patches.
//!
//! A Bézier curve is its list of control points. A *bezpath* strings several
//! together, sharing the point where one ends and the next begins, so a cubic
//! path has `3n + 1` points. A patch is a square grid of control points.

use mlua::{Lua, Result as LuaResult, Value as LuaValue};

use crate::bosl::value::{Args, PureFn, Val, num_list, register_all, v3};
use crate::scad_export::ScadNode;

const EPS: f64 = 1e-12;

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

fn unit(a: [f64; 3]) -> [f64; 3] {
  let n = norm(a);
  if n < EPS { [0.0; 3] } else { scale(a, 1.0 / n) }
}

/// The width of a curve's control points, so 2D input gives 2D output.
fn curve_dim(a: &Args, name: &str) -> usize {
  a.val(name)
    .and_then(|v| v.as_matrix())
    .and_then(|m| m.first().map(|p| p.len()))
    .unwrap_or(3)
    .clamp(2, 3)
}

fn out_point(p: [f64; 3], dim: usize) -> Val {
  Val::vec(p[..dim].to_vec())
}

/// The point on a Bézier curve at parameter `u`.
///
/// De Casteljau's construction: repeatedly interpolate between neighbouring
/// control points until one is left. It costs a little more than evaluating
/// the polynomial directly, but stays accurate for high degrees where the
/// binomial coefficients would otherwise swamp the result.
pub fn bezier_at(ctrl: &[[f64; 3]], u: f64) -> [f64; 3] {
  if ctrl.is_empty() {
    return [0.0; 3];
  }
  let mut pts = ctrl.to_vec();
  while pts.len() > 1 {
    pts = pts
      .windows(2)
      .map(|w| add(scale(w[0], 1.0 - u), scale(w[1], u)))
      .collect();
  }
  pts[0]
}

/// The control points of the derivative curve, one degree lower.
fn derivative_ctrl(ctrl: &[[f64; 3]]) -> Vec<[f64; 3]> {
  if ctrl.len() < 2 {
    return vec![[0.0; 3]];
  }
  let n = (ctrl.len() - 1) as f64;
  ctrl.windows(2).map(|w| scale(sub(w[1], w[0]), n)).collect()
}

fn nth_derivative(ctrl: &[[f64; 3]], order: usize) -> Vec<[f64; 3]> {
  let mut c = ctrl.to_vec();
  for _ in 0..order {
    c = derivative_ctrl(&c);
  }
  c
}

/// The parameter values a curve is sampled at.
fn samples(steps: usize, endpoint: bool) -> Vec<f64> {
  let n = steps.max(1);
  let d = if endpoint { n } else { n + 1 };
  (0..=n)
    .take(if endpoint { n + 1 } else { n })
    .map(|i| i as f64 / d as f64)
    .collect()
}

/// Read a curve's control points.
fn read_curve(a: &Args, name: &str) -> LuaResult<Vec<[f64; 3]>> {
  match a.val(name).and_then(|v| v.as_matrix()) {
    Some(m) if !m.is_empty() => Ok(m.iter().map(|p| v3(p)).collect()),
    _ => a.err(format!("{name} must be a list of control points")),
  }
}

/// The curve of a bezpath's `i`th segment.
fn segment(bezpath: &[[f64; 3]], i: usize, degree: usize) -> Vec<[f64; 3]> {
  let start = i * degree;
  bezpath[start..(start + degree + 1).min(bezpath.len())].to_vec()
}

fn segment_count(bezpath: &[[f64; 3]], degree: usize) -> usize {
  if bezpath.len() < degree + 1 {
    0
  } else {
    (bezpath.len() - 1) / degree
  }
}

// ---------------------------------------------------------------------------
// Evaluating a curve
// ---------------------------------------------------------------------------

fn bezier_points(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let ctrl = read_curve(a, "curve")?;
  let dim = curve_dim(a, "curve");
  match a.need_val("u")? {
    Val::Num(u) => out_point(bezier_at(&ctrl, u), dim).to_lua(lua),
    other => match other.as_vec() {
      Some(us) => {
        Val::list(us.iter().map(|u| out_point(bezier_at(&ctrl, *u), dim)))
          .to_lua(lua)
      }
      None => a.err("u must be a number or a list of numbers"),
    },
  }
}

fn bezier_curve(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let ctrl = read_curve(a, "bezier")?;
  let dim = curve_dim(a, "bezier");
  let steps = a.int("splinesteps").unwrap_or(16).max(1) as usize;
  let endpoint = a.bool_or("endpoint", true);
  Val::list(
    samples(steps, endpoint)
      .iter()
      .map(|u| out_point(bezier_at(&ctrl, *u), dim)),
  )
  .to_lua(lua)
}

fn bezier_derivative(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let ctrl = read_curve(a, "bezier")?;
  let dim = curve_dim(a, "bezier");
  let order = a.int("order").unwrap_or(1).max(0) as usize;
  let d = nth_derivative(&ctrl, order);
  match a.need_val("u")? {
    Val::Num(u) => out_point(bezier_at(&d, u), dim).to_lua(lua),
    other => match other.as_vec() {
      Some(us) => {
        Val::list(us.iter().map(|u| out_point(bezier_at(&d, *u), dim)))
          .to_lua(lua)
      }
      None => a.err("u must be a number or a list of numbers"),
    },
  }
}

fn bezier_tangent(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let ctrl = read_curve(a, "bezier")?;
  let dim = curve_dim(a, "bezier");
  let d = derivative_ctrl(&ctrl);
  match a.need_val("u")? {
    Val::Num(u) => out_point(unit(bezier_at(&d, u)), dim).to_lua(lua),
    other => match other.as_vec() {
      Some(us) => {
        Val::list(us.iter().map(|u| out_point(unit(bezier_at(&d, *u)), dim)))
          .to_lua(lua)
      }
      None => a.err("u must be a number or a list of numbers"),
    },
  }
}

/// How sharply the curve bends, as one over the radius of the circle that
/// best fits it.
fn curvature_at(ctrl: &[[f64; 3]], u: f64) -> f64 {
  let d1 = bezier_at(&nth_derivative(ctrl, 1), u);
  let d2 = bezier_at(&nth_derivative(ctrl, 2), u);
  let speed = norm(d1);
  if speed < EPS {
    return 0.0;
  }
  norm(cross(d1, d2)) / speed.powi(3)
}

fn bezier_curvature(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let ctrl = read_curve(a, "bezier")?;
  match a.need_val("u")? {
    Val::Num(u) => Ok(LuaValue::Number(curvature_at(&ctrl, u))),
    other => match other.as_vec() {
      Some(us) => num_list(
        lua,
        &us
          .iter()
          .map(|u| curvature_at(&ctrl, *u))
          .collect::<Vec<_>>(),
      ),
      None => a.err("u must be a number or a list of numbers"),
    },
  }
}

/// The arc length of part of a curve.
///
/// The curve is split until each piece is straight enough that its chord is
/// within `max_deflect` of it, which converges far faster than sampling at a
/// fixed rate and is what lets a nearly-straight curve cost almost nothing.
fn length_of(
  ctrl: &[[f64; 3]],
  start_u: f64,
  end_u: f64,
  max_deflect: f64,
  depth: usize,
) -> f64 {
  let segs = (ctrl.len() * 2).max(2);
  let pts: Vec<[f64; 3]> = (0..=segs)
    .map(|i| {
      let u = start_u + (end_u - start_u) * i as f64 / segs as f64;
      bezier_at(ctrl, u)
    })
    .collect();
  let deflection = pts
    .windows(3)
    .map(|w| {
      let mid = scale(add(w[0], w[2]), 0.5);
      norm(sub(w[1], mid))
    })
    .fold(0.0f64, f64::max);

  let chord: f64 = pts.windows(2).map(|w| norm(sub(w[1], w[0]))).sum();
  if deflection <= max_deflect || depth == 0 {
    return chord;
  }
  let mid_u = (start_u + end_u) / 2.0;
  length_of(ctrl, start_u, mid_u, max_deflect, depth - 1)
    + length_of(ctrl, mid_u, end_u, max_deflect, depth - 1)
}

fn bezier_length(_lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let ctrl = read_curve(a, "bezier")?;
  let start = a.num_or("start_u", 0.0);
  let end = a.num_or("end_u", 1.0);
  let deflect = a.num_or("max_deflect", 0.01);
  Ok(LuaValue::Number(length_of(&ctrl, start, end, deflect, 24)))
}

/// The parameter of the point on a curve nearest `pt`.
fn closest_u(ctrl: &[[f64; 3]], pt: [f64; 3], max_err: f64) -> f64 {
  // Scan coarsely for every local minimum, then narrow each one; a single
  // descent would settle into whichever basin it started in.
  let steps = (ctrl.len() * 6).max(24);
  let dist = |u: f64| norm(sub(bezier_at(ctrl, u), pt));
  let mut best_u = 0.0;
  let mut best_d = f64::INFINITY;
  for i in 0..=steps {
    let u0 = i as f64 / steps as f64;
    let d = dist(u0);
    if d < best_d {
      best_d = d;
      best_u = u0;
    }
  }
  // Golden-section search inside the bracket around the best sample.
  let span = 1.0 / steps as f64;
  let (mut lo, mut hi) = ((best_u - span).max(0.0), (best_u + span).min(1.0));
  let phi = (5f64.sqrt() - 1.0) / 2.0;
  while hi - lo > max_err {
    let c = hi - (hi - lo) * phi;
    let d = lo + (hi - lo) * phi;
    if dist(c) < dist(d) {
      hi = d;
    } else {
      lo = c;
    }
  }
  (lo + hi) / 2.0
}

fn bezier_closest_point(_lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let ctrl = read_curve(a, "bezier")?;
  let pt = v3(&a.need_vec("pt")?);
  let max_err = a.num_or("max_err", 0.01);
  Ok(LuaValue::Number(closest_u(&ctrl, pt, max_err)))
}

/// Where a 2D Bézier crosses a line, as the parameters at which it does.
fn bezier_line_intersection(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let ctrl = read_curve(a, "bezier")?;
  let line = a.need_matrix("line")?;
  if line.len() != 2 {
    return a.err("line must be two points");
  }
  let (p0, p1) = (v3(&line[0]), v3(&line[1]));
  // Distance from the line is linear in the point, so substituting the curve
  // gives a polynomial in u whose roots are the crossings.
  let n = [p0[1] - p1[1], p1[0] - p0[0], 0.0];
  let offset = dot(n, p0);

  let degree = ctrl.len() - 1;
  // Bernstein coefficients of the signed distance, converted to the power
  // basis so the root finder can take them.
  let mut power = vec![0.0; ctrl.len()];
  for (i, c) in ctrl.iter().enumerate() {
    let value = dot(n, *c) - offset;
    // Each control point contributes its Bernstein basis polynomial.
    for k in 0..=(degree - i) {
      let sign = if k % 2 == 0 { 1.0 } else { -1.0 };
      power[i + k] += value
        * binomial(degree, i) as f64
        * binomial(degree - i, k) as f64
        * sign;
    }
  }
  power.reverse();

  let roots = crate::bosl::math::real_roots_of(&power);
  let mut us: Vec<f64> = roots
    .into_iter()
    .filter(|u| (-1e-9..=1.0 + 1e-9).contains(u))
    .map(|u| u.clamp(0.0, 1.0))
    .collect();
  us.sort_by(f64::total_cmp);
  us.dedup_by(|x, y| (*x - *y).abs() < 1e-9);
  num_list(lua, &us)
}

fn binomial(n: usize, k: usize) -> u64 {
  let mut c = 1u64;
  for i in 0..k.min(n - k) {
    c = c * (n - i) as u64 / (i + 1) as u64;
  }
  c
}

// ---------------------------------------------------------------------------
// Bezier paths
// ---------------------------------------------------------------------------

fn read_bezpath(a: &Args) -> LuaResult<(Vec<[f64; 3]>, usize, usize)> {
  let path = read_curve(a, "bezpath")?;
  let degree = a.int("N").unwrap_or(3).max(1) as usize;
  if path.len() < degree + 1 || (path.len() - 1) % degree != 0 {
    return a.err(format!(
      "a degree {degree} bezier path needs a multiple of {degree} points, plus one"
    ));
  }
  let dim = curve_dim(a, "bezpath");
  Ok((path, degree, dim))
}

fn bezpath_points(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let (path, degree, dim) = read_bezpath(a)?;
  let i = a.need_num("curveind")? as usize;
  if i >= segment_count(&path, degree) {
    return a.err("curveind is past the end of the path");
  }
  let curve = segment(&path, i, degree);
  match a.need_val("u")? {
    Val::Num(u) => out_point(bezier_at(&curve, u), dim).to_lua(lua),
    other => match other.as_vec() {
      Some(us) => {
        Val::list(us.iter().map(|u| out_point(bezier_at(&curve, *u), dim)))
          .to_lua(lua)
      }
      None => a.err("u must be a number or a list of numbers"),
    },
  }
}

fn bezpath_curve(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let (path, degree, dim) = read_bezpath(a)?;
  let steps = a.int("splinesteps").unwrap_or(16).max(1) as usize;
  let endpoint = a.bool_or("endpoint", true);
  let segs = segment_count(&path, degree);

  let mut out: Vec<Val> = Vec::new();
  for s in 0..segs {
    let curve = segment(&path, s, degree);
    // Each segment stops one step short so it does not repeat the point the
    // next one starts on.
    for i in 0..steps {
      let u = i as f64 / steps as f64;
      out.push(out_point(bezier_at(&curve, u), dim));
    }
  }
  if endpoint {
    out.push(out_point(path[path.len() - 1], dim));
  }
  Val::List(out).to_lua(lua)
}

fn bezpath_length(_lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let (path, degree, _) = read_bezpath(a)?;
  let deflect = a.num_or("max_deflect", 0.001);
  let total: f64 = (0..segment_count(&path, degree))
    .map(|s| length_of(&segment(&path, s, degree), 0.0, 1.0, deflect, 24))
    .sum();
  Ok(LuaValue::Number(total))
}

fn bezpath_closest_point(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let (path, degree, _) = read_bezpath(a)?;
  let pt = v3(&a.need_vec("pt")?);
  let max_err = a.num_or("max_err", 0.01);
  let mut best = (0usize, 0.0f64, f64::INFINITY);
  for s in 0..segment_count(&path, degree) {
    let curve = segment(&path, s, degree);
    let u = closest_u(&curve, pt, max_err);
    let d = norm(sub(bezier_at(&curve, u), pt));
    if d < best.2 {
      best = (s, u, d);
    }
  }
  Val::list([Val::Num(best.0 as f64), Val::Num(best.1)]).to_lua(lua)
}

/// Fit a smooth Bézier path through a list of points.
fn path_to_bezpath(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let path = crate::bosl::paths::read_path(a, "path")?;
  let dim = curve_dim(a, "path");
  let closed = a.bool_or("closed", false);
  if path.len() < 2 {
    return a.err("the path needs at least two points");
  }
  let n = path.len();

  // The handles point along the direction between each point's neighbours,
  // which is what makes the joins smooth.
  let tangent = |i: usize| -> [f64; 3] {
    let prev = if i == 0 {
      if closed { path[n - 1] } else { path[0] }
    } else {
      path[i - 1]
    };
    let next = if i == n - 1 {
      if closed { path[0] } else { path[n - 1] }
    } else {
      path[i + 1]
    };
    unit(sub(next, prev))
  };

  // How far the handles reach: either a fixed size, or a fraction of the
  // segment they belong to.
  let relsize = a.num("relsize");
  let size = a.num("size");
  let reach = |i: usize, j: usize| -> f64 {
    let seg = norm(sub(path[j], path[i]));
    match (size, relsize) {
      (Some(s), _) => s,
      (None, Some(r)) => seg * r,
      (None, None) => seg * 0.1 / 0.3,
    }
  };

  let segs = if closed { n } else { n - 1 };
  let mut out: Vec<Val> = vec![out_point(path[0], dim)];
  for s in 0..segs {
    let i = s;
    let j = (s + 1) % n;
    let d = reach(i, j) / 3.0 * 3.0;
    out.push(out_point(add(path[i], scale(tangent(i), d / 3.0)), dim));
    out.push(out_point(sub(path[j], scale(tangent(j), d / 3.0)), dim));
    out.push(out_point(path[j], dim));
  }
  Val::List(out).to_lua(lua)
}

/// Close a 2D bezier path down onto an axis, making a shape to revolve.
fn bezpath_close_to_axis(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let (path, degree, _) = read_bezpath(a)?;
  let axis = a.string("axis").unwrap_or_else(|| "X".to_string());
  let sp = path[0];
  let ep = path[path.len() - 1];
  let foot = |p: [f64; 3]| -> [f64; 3] {
    if axis == "X" {
      [p[0], 0.0, 0.0]
    } else {
      [0.0, p[1], 0.0]
    }
  };
  // Straight runs in and out are written as Bézier segments too, so the
  // result is still a valid path of the same degree.
  let run = |from: [f64; 3], to: [f64; 3], include_end: bool| -> Vec<Val> {
    let count = if include_end { degree + 1 } else { degree };
    (0..count)
      .map(|i| {
        let t = i as f64 / degree as f64;
        Val::vec(add(scale(from, 1.0 - t), scale(to, t))[..2].to_vec())
      })
      .collect()
  };

  let mut out: Vec<Val> = Vec::new();
  out.extend(run(foot(sp), sp, false));
  out.extend(
    path[..path.len() - 1]
      .iter()
      .map(|p| Val::vec(p[..2].to_vec())),
  );
  out.extend(run(ep, foot(ep), false));
  out.extend(run(foot(ep), foot(sp), true));
  Val::List(out).to_lua(lua)
}

/// Offset a 2D bezier path sideways by a fixed distance.
fn bezpath_offset(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let offset = match a.val("offset") {
    Some(Val::Num(d)) => [d, 0.0, 0.0],
    Some(other) => match other.as_vec() {
      Some(v) => v3(&v),
      None => return a.err("offset must be a distance or a 2-vector"),
    },
    None => return a.err("offset is required"),
  };
  let (path, _, _) = read_bezpath(a)?;
  // The offset path is the original shifted, then joined back to it, which
  // is what BOSL2 uses to make a closed band from an open curve.
  let mut out: Vec<Val> =
    path.iter().map(|p| Val::vec(p[..2].to_vec())).collect();
  out.extend(
    path
      .iter()
      .rev()
      .map(|p| Val::vec(add(*p, offset)[..2].to_vec())),
  );
  Val::List(out).to_lua(lua)
}

// ---------------------------------------------------------------------------
// Patches
// ---------------------------------------------------------------------------

/// Read a patch: a square grid of control points.
fn read_patch(a: &Args, name: &str) -> LuaResult<Vec<Vec<[f64; 3]>>> {
  let Some(rows) = a.val(name).and_then(|v| v.as_list().map(|s| s.to_vec()))
  else {
    return a.err(format!("{name} must be a grid of control points"));
  };
  let mut out = Vec::with_capacity(rows.len());
  for row in &rows {
    match row.as_matrix() {
      Some(m) => out.push(m.iter().map(|p| v3(p)).collect()),
      None => return a.err(format!("{name} must be a grid of control points")),
    }
  }
  Ok(out)
}

/// The point on a patch at `(u, v)`, by running the curve construction in
/// one direction and then the other.
fn patch_at(patch: &[Vec<[f64; 3]>], u: f64, v: f64) -> [f64; 3] {
  let column: Vec<[f64; 3]> =
    patch.iter().map(|row| bezier_at(row, u)).collect();
  bezier_at(&column, v)
}

fn is_bezier_patch(_lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let ok = match a.val("x").and_then(|v| v.as_list().map(|s| s.to_vec())) {
    Some(rows) if rows.len() >= 2 => {
      let widths: Vec<usize> = rows
        .iter()
        .filter_map(|r| r.as_matrix().map(|m| m.len()))
        .collect();
      widths.len() == rows.len()
        && widths.iter().all(|w| *w == widths[0])
        && widths[0] >= 2
    }
    _ => false,
  };
  Ok(LuaValue::Boolean(ok))
}

fn bezier_patch_points(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let patch = read_patch(a, "patch")?;
  let dim = 3;
  let us = a.need_val("u")?;
  let vs = a.need_val("v")?;
  match (us, vs) {
    (Val::Num(u), Val::Num(v)) => {
      out_point(patch_at(&patch, u, v), dim).to_lua(lua)
    }
    (u, v) => {
      let (Some(us), Some(vs)) = (u.as_vec(), v.as_vec()) else {
        return a.err("u and v must both be numbers or both be lists");
      };
      // A grid of parameters gives a grid of points.
      Val::list(vs.iter().map(|v| {
        Val::list(us.iter().map(|u| out_point(patch_at(&patch, *u, *v), dim)))
      }))
      .to_lua(lua)
    }
  }
}

fn bezier_patch_normals(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let patch = read_patch(a, "patch")?;
  let normal_at = |u: f64, v: f64| -> [f64; 3] {
    // The surface normal is the cross product of the two tangents, taken by
    // finite difference so a patch of any degree is handled the same way.
    let h = 1e-5;
    let du = sub(
      patch_at(&patch, (u + h).min(1.0), v),
      patch_at(&patch, (u - h).max(0.0), v),
    );
    let dv = sub(
      patch_at(&patch, u, (v + h).min(1.0)),
      patch_at(&patch, u, (v - h).max(0.0)),
    );
    unit(cross(du, dv))
  };
  match (a.need_val("u")?, a.need_val("v")?) {
    (Val::Num(u), Val::Num(v)) => Val::vec(normal_at(u, v)).to_lua(lua),
    (u, v) => {
      let (Some(us), Some(vs)) = (u.as_vec(), v.as_vec()) else {
        return a.err("u and v must both be numbers or both be lists");
      };
      Val::list(
        vs.iter()
          .map(|v| Val::list(us.iter().map(|u| Val::vec(normal_at(*u, *v))))),
      )
      .to_lua(lua)
    }
  }
}

fn bezier_patch_reverse(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let patch = read_patch(a, "patch")?;
  // Reversing each row flips the surface's facing without moving it.
  Val::list(
    patch
      .iter()
      .map(|row| Val::list(row.iter().rev().map(|p| Val::vec(*p)))),
  )
  .to_lua(lua)
}

/// Whether a patch is flat enough to draw as two triangles.
fn bezier_patch_flat(_lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let patch = read_patch(a, "patch")?;
  let eps = a.num_or("eps", 1e-4);
  if patch.len() < 2 || patch[0].len() < 2 {
    return Ok(LuaValue::Boolean(true));
  }
  // Measure every control point against the plane through the corners.
  let c0 = patch[0][0];
  let c1 = patch[0][patch[0].len() - 1];
  let c2 = patch[patch.len() - 1][0];
  let n = unit(cross(sub(c1, c0), sub(c2, c0)));
  if norm(n) < EPS {
    return Ok(LuaValue::Boolean(true));
  }
  let size = norm(sub(c1, c0)).max(norm(sub(c2, c0))).max(EPS);
  let flat = patch
    .iter()
    .flatten()
    .all(|p| (dot(n, sub(*p, c0))).abs() / size <= eps);
  Ok(LuaValue::Boolean(flat))
}

/// Turn a patch, or a list of them, into a mesh.
fn bezier_vnf(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let patches = read_patch_list(a)?;
  let steps = a.int("splinesteps").unwrap_or(16).max(1) as usize;
  let mut vnf = crate::bosl::vnf::Vnf::new();
  for patch in &patches {
    let rows: Vec<Vec<[f64; 3]>> = (0..=steps)
      .map(|j| {
        let v = j as f64 / steps as f64;
        (0..=steps)
          .map(|i| patch_at(patch, i as f64 / steps as f64, v))
          .collect()
      })
      .collect();
    vnf.join(&crate::bosl::vnf::Vnf::vertex_array(
      &rows,
      crate::bosl::vnf::Caps::NONE,
      false,
      false,
    ));
  }
  vnf_to_lua(lua, &vnf.reversed())
}

/// Accept either one patch or a list of them.
fn read_patch_list(a: &Args) -> LuaResult<Vec<Vec<Vec<[f64; 3]>>>> {
  let Some(v) = a.val("patches").or_else(|| a.val("patch")) else {
    return a.err("a patch is required");
  };
  // A patch is a grid of points; a list of patches is one level deeper.
  if let Some(rows) = v.as_list() {
    if rows.first().and_then(|r| r.as_matrix()).is_some() {
      let mut patch = Vec::new();
      for row in rows {
        match row.as_matrix() {
          Some(m) => patch.push(m.iter().map(|p| v3(p)).collect()),
          None => return a.err("a patch must be a grid of control points"),
        }
      }
      return Ok(vec![patch]);
    }
    let mut out = Vec::new();
    for p in rows {
      let Some(prows) = p.as_list() else {
        return a.err("a patch must be a grid of control points");
      };
      let mut patch = Vec::new();
      for row in prows {
        match row.as_matrix() {
          Some(m) => patch.push(m.iter().map(|q| v3(q)).collect()),
          None => return a.err("a patch must be a grid of control points"),
        }
      }
      out.push(patch);
    }
    return Ok(out);
  }
  a.err("a patch must be a grid of control points")
}

/// A patch with one edge collapsed to a point, for closing off a corner.
fn bezier_vnf_degenerate_patch(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let patch = read_patch(a, "patch")?;
  let steps = a.int("splinesteps").unwrap_or(16).max(1) as usize;
  let rows: Vec<Vec<[f64; 3]>> = (0..=steps)
    .map(|j| {
      let v = j as f64 / steps as f64;
      (0..=steps)
        .map(|i| patch_at(&patch, i as f64 / steps as f64, v))
        .collect()
    })
    .collect();
  // Welding removes the duplicated points along the collapsed edge, which
  // is what makes the result a usable mesh rather than a folded sheet.
  let vnf = crate::bosl::vnf::Vnf::vertex_array(
    &rows,
    crate::bosl::vnf::Caps::NONE,
    false,
    false,
  )
  .merged(1e-9);
  vnf_to_lua(lua, &vnf.reversed())
}

/// Hand a mesh back to Lua in BOSL2's `[points, faces]` form, which winds
/// its faces the way `polyhedron()` wants.
pub fn vnf_to_lua(
  lua: &Lua,
  vnf: &crate::bosl::vnf::Vnf,
) -> LuaResult<LuaValue> {
  Val::list([
    Val::list(vnf.points.iter().map(|p| Val::vec(*p))),
    Val::list(
      vnf
        .faces
        .iter()
        .map(|f| Val::vec(f.iter().map(|i| *i as f64))),
    ),
  ])
  .to_lua(lua)
}

// ---------------------------------------------------------------------------
// Writing a cubic Bézier path by its joints
// ---------------------------------------------------------------------------

/// The direction a `bez_*` joint leaves in, however it was given.
///
/// Either as a vector — whose length sets the handle unless a radius says
/// otherwise — or as a compass bearing with an optional elevation, which is
/// how a 3D joint is aimed without writing the vector out.
fn joint_direction(
  a: &Args,
  angle: &str,
  radius: Option<f64>,
  elevation: &str,
) -> LuaResult<Option<[f64; 3]>> {
  let Some(raw) = a.raw(angle) else {
    return Ok(None);
  };
  if let Some(v) = crate::bosl::args::as_nums(raw) {
    if v.len() < 2 {
      return a.err(format!("{angle} must be a direction or an angle"));
    }
    let dir = v3(&v);
    let len = norm(dir);
    if len < EPS {
      return a.err(format!("{angle} must not be zero"));
    }
    let r = radius.unwrap_or(len);
    return Ok(Some(scale(dir, r / len)));
  }
  let Some(theta) = crate::bosl::args::as_num(raw) else {
    return a.err(format!("{angle} must be a direction or an angle"));
  };
  let Some(r) = radius else {
    return a.err(format!("a radius is needed alongside the angle {angle}"));
  };
  let phi = a.num_or(elevation, 90.0);
  let (st, ct) = theta.to_radians().sin_cos();
  let (sp, cp) = phi.to_radians().sin_cos();
  Ok(Some([r * ct * sp, r * st * sp, r * cp]))
}

fn joint_point(a: &Args) -> LuaResult<[f64; 3]> {
  match a.points3("pt").as_deref() {
    Some([p]) => Ok(*p),
    _ => match a.vec3("pt") {
      Some(p) => Ok(p),
      None => a.err("pt must be a point"),
    },
  }
}

/// The two control points that start a cubic Bézier path.
fn bez_begin(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let pt = joint_point(a)?;
  let Some(d) = joint_direction(a, "a", a.num("r"), "p")? else {
    return a.err("a direction or angle is required");
  };
  let dim =
    if a.vec3("pt").map(|_| a.nums("pt").map(|v| v.len())) == Some(Some(2)) {
      2
    } else {
      3
    };
  Val::list([out_point(pt, dim), out_point(add(pt, d), dim)]).to_lua(lua)
}

/// The two control points that finish a cubic Bézier path.
fn bez_end(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let pt = joint_point(a)?;
  let Some(d) = joint_direction(a, "a", a.num("r"), "p")? else {
    return a.err("a direction or angle is required");
  };
  let dim = a.nums("pt").map(|v| v.len()).unwrap_or(3).min(3);
  Val::list([out_point(add(pt, d), dim), out_point(pt, dim)]).to_lua(lua)
}

/// The three control points of a smooth joint, where the path runs straight
/// through with the same tangent either side.
fn bez_tang(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let pt = joint_point(a)?;
  let dim = a.nums("pt").map(|v| v.len()).unwrap_or(3).min(3);
  let r1 = a.num("r1");
  let out = joint_direction(a, "a", r1, "p")?;
  let Some(out_dir) = out else {
    return a.err("a direction or angle is required");
  };
  // The handle behind the point mirrors the one in front, at its own length.
  let back_len = r1.unwrap_or_else(|| norm(out_dir));
  let r2 = a.num("r2").unwrap_or(back_len);
  let unit_dir = unit(out_dir);
  Val::list([
    out_point(sub(pt, scale(unit_dir, back_len)), dim),
    out_point(pt, dim),
    out_point(add(pt, scale(unit_dir, r2)), dim),
  ])
  .to_lua(lua)
}

/// The three control points of a corner, where the path arrives along one
/// direction and leaves along another.
fn bez_joint(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let pt = joint_point(a)?;
  let dim = a.nums("pt").map(|v| v.len()).unwrap_or(3).min(3);
  let Some(d1) = joint_direction(a, "a1", a.num("r1"), "p1")? else {
    return a.err("a1 is required");
  };
  let Some(d2) = joint_direction(a, "a2", a.num("r2"), "p2")? else {
    return a.err("a2 is required");
  };
  Val::list([
    out_point(add(pt, d1), dim),
    out_point(pt, dim),
    out_point(add(pt, d2), dim),
  ])
  .to_lua(lua)
}

/// Turn a polyline into a cubic Bézier path that rounds off its corners.
///
/// Each corner gets two control points set back along its two edges, so the
/// curve leaves and rejoins the polyline smoothly. `size` measures that
/// setback outright; `relsize` measures it as a fraction of the shorter edge.
fn path_to_bezcornerpath(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let path = read_curve(a, "path")?;
  let dim = curve_dim(a, "path");
  let closed = a.bool_or("closed", false);
  let n = path.len();
  if n < 2 {
    return a.err("path must have at least two points");
  }
  if a.has("size") && a.has("relsize") {
    return a.err("give either size or relsize, not both");
  }
  let relative = !a.has("size");
  let amount = a.num("size").or_else(|| a.num("relsize")).unwrap_or(0.5);
  if amount <= 0.0 {
    return a.err("size or relsize must be greater than zero");
  }

  let mut out: Vec<Val> = Vec::new();
  let corner_handles = |i: usize| -> ([f64; 3], [f64; 3]) {
    let here = path[i];
    let prev = path[(i + n - 1) % n];
    let next = path[(i + 1) % n];
    let back = sub(prev, here);
    let fwd = sub(next, here);
    let reach = |v: [f64; 3]| {
      let len = norm(v);
      if len < EPS {
        return [0.0; 3];
      }
      let d = if relative { len * amount / 2.0 } else { amount };
      scale(v, d.min(len) / len)
    };
    (add(here, reach(back)), add(here, reach(fwd)))
  };

  if closed {
    for (i, here) in path.iter().enumerate() {
      let (back, fwd) = corner_handles(i);
      out.push(out_point(back, dim));
      out.push(out_point(*here, dim));
      out.push(out_point(fwd, dim));
    }
    // A closed path comes back round to where it started.
    out.rotate_left(1);
    let first = out[0].clone();
    out.push(first);
  } else {
    out.push(out_point(path[0], dim));
    for (i, here) in path.iter().enumerate().take(n - 1).skip(1) {
      let (back, fwd) = corner_handles(i);
      out.push(out_point(back, dim));
      out.push(out_point(*here, dim));
      out.push(out_point(fwd, dim));
    }
    out.push(out_point(path[n - 1], dim));
  }
  Val::List(out).to_lua(lua)
}

/// Give a Bézier patch a thickness, so it becomes a solid shell.
fn bezier_sheet(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let patch = read_patch(a, "patch")?;
  let thickness = a.need_num("thickness")?;
  if thickness.abs() < EPS {
    return a.err("thickness must not be zero");
  }
  let steps = a.int("splinesteps").unwrap_or(16).max(1) as usize;

  let normal_at = |u: f64, v: f64| -> [f64; 3] {
    let h = 1e-5;
    let du = sub(
      patch_at(&patch, (u + h).min(1.0), v),
      patch_at(&patch, (u - h).max(0.0), v),
    );
    let dv = sub(
      patch_at(&patch, u, (v + h).min(1.0)),
      patch_at(&patch, u, (v - h).max(0.0)),
    );
    unit(cross(du, dv))
  };

  // The surface, then the same surface pushed out along its own normals.
  let mut front: Vec<Vec<[f64; 3]>> = Vec::with_capacity(steps + 1);
  let mut back: Vec<Vec<[f64; 3]>> = Vec::with_capacity(steps + 1);
  for j in 0..=steps {
    let v = 1.0 - j as f64 / steps as f64;
    let mut row_f = Vec::with_capacity(steps + 1);
    let mut row_b = Vec::with_capacity(steps + 1);
    for i in 0..=steps {
      let u = i as f64 / steps as f64;
      let p = patch_at(&patch, u, v);
      row_f.push(p);
      row_b.push(add(p, scale(normal_at(u, v), thickness)));
    }
    front.push(row_f);
    back.push(row_b);
  }

  // The two sheets meet all the way round their edges, so stacking the front
  // rows, then the back rows reversed, closes the solid without a seam.
  let mut rows = front;
  rows.extend(back.into_iter().rev());
  let vnf = crate::bosl::vnf::Vnf::vertex_array(
    &rows,
    crate::bosl::vnf::Caps::NONE,
    false,
    true,
  );
  crate::bosl::vnf_lua::write_vnf(lua, &vnf)
}

/// Draw a Bézier path with its control points, so it can be looked at.
fn debug_bezier(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let bezpath = read_curve(a, "bezpath")?;
  let width = a.num_or("width", 1.0);
  let degree = a.int("N").unwrap_or(3).max(1) as usize;
  if bezpath.len() % degree != 1 {
    return a.err(format!(
      "a degree {degree} bezier path needs a multiple of {degree} points, \
       plus one"
    ));
  }
  // The curve itself, and the control polygon it is steered by.
  let mut curve: Vec<[f64; 3]> = Vec::new();
  for seg in 0..(bezpath.len() - 1) / degree {
    let ctrl = &bezpath[seg * degree..seg * degree + degree + 1];
    for k in 0..=16 {
      curve.push(bezier_at(ctrl, k as f64 / 16.0));
    }
  }
  let node = ScadNode::Union(
    polyline_bars(&curve, width)
      .into_iter()
      .chain(polyline_bars(&bezpath, width / 2.0))
      .collect(),
  );
  as_debug_geometry(lua, "debug_bezier", a, node)
}

/// Draw a list of Bézier patches with their control nets.
fn debug_bezier_patches(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let patches = read_patch_list(a)?;
  let steps = a.int("splinesteps").unwrap_or(16).max(1) as usize;
  let show_cps = a.bool_or("showcps", true);
  let size = a.num_or("size", 1.0);

  let mut parts: Vec<ScadNode> = Vec::new();
  for patch in &patches {
    let rows: Vec<Vec<[f64; 3]>> = (0..=steps)
      .map(|j| {
        let v = j as f64 / steps as f64;
        (0..=steps)
          .map(|i| patch_at(patch, i as f64 / steps as f64, v))
          .collect()
      })
      .collect();
    parts.push(
      crate::bosl::vnf::Vnf::vertex_array(
        &rows,
        crate::bosl::vnf::Caps::NONE,
        false,
        false,
      )
      .to_node(),
    );
    if show_cps {
      for row in patch {
        parts.extend(polyline_bars(row, size / 3.0));
      }
    }
  }
  as_debug_geometry(lua, "debug_bezier_patches", a, ScadNode::Union(parts))
}

/// A run of thin bars along a polyline, for drawing it.
fn polyline_bars(path: &[[f64; 3]], width: f64) -> Vec<ScadNode> {
  path
    .windows(2)
    .filter_map(|w| {
      let d = sub(w[1], w[0]);
      let len = norm(d);
      if len < EPS {
        return None;
      }
      let yaw = d[1].atan2(d[0]).to_degrees();
      let pitch = (d[2] / len).clamp(-1.0, 1.0).acos().to_degrees();
      Some(ScadNode::Translate {
        x: w[0][0] as f32,
        y: w[0][1] as f32,
        z: w[0][2] as f32,
        child: Box::new(ScadNode::Rotate {
          x: 0.0,
          y: pitch as f32,
          z: yaw as f32,
          child: Box::new(ScadNode::Cylinder {
            r1: (width / 2.0) as f32,
            r2: (width / 2.0) as f32,
            h: len as f32,
            center: false,
            segments: 8,
          }),
        }),
      })
    })
    .collect()
}

fn as_debug_geometry(
  lua: &Lua,
  name: &'static str,
  a: &Args,
  node: ScadNode,
) -> LuaResult<LuaValue> {
  let scad = crate::bosl::bosl_node_with_children(
    "std.scad",
    name,
    a.scad_args().to_string(),
    vec![],
    Some(node),
  );
  Ok(LuaValue::UserData(lua.create_userdata(
    crate::geometry::CsgGeometry {
      name: None,
      mesh: None,
      color: None,
      material: None,
      scad: Some(scad),
    },
  )?))
}

pub fn register(lua: &Lua, bosl: &mlua::Table) -> LuaResult<()> {
  register_all(
    lua,
    bosl,
    &[
      ("bez_begin", &["pt", "a", "r", "p"], bez_begin as PureFn),
      ("bez_end", &["pt", "a", "r", "p"], bez_end),
      ("bez_tang", &["pt", "a", "r1", "r2", "p"], bez_tang),
      (
        "bez_joint",
        &["pt", "a1", "a2", "r1", "r2", "p1", "p2"],
        bez_joint,
      ),
      (
        "path_to_bezcornerpath",
        &["path", "closed", "size", "relsize"],
        path_to_bezcornerpath,
      ),
      (
        "bezier_sheet",
        &["patch", "thickness", "splinesteps", "style"],
        bezier_sheet,
      ),
      ("debug_bezier", &["bezpath", "width", "N"], debug_bezier),
      (
        "debug_bezier_patches",
        &[
          "patches",
          "size",
          "splinesteps",
          "showcps",
          "showdots",
          "showpatch",
          "convexity",
          "style",
        ],
        debug_bezier_patches,
      ),
    ],
  )?;
  register_all(
    lua,
    bosl,
    &[
      ("bezier_points", &["curve", "u"], bezier_points as PureFn),
      (
        "bezier_curve",
        &["bezier", "splinesteps", "endpoint"],
        bezier_curve,
      ),
      (
        "bezier_derivative",
        &["bezier", "u", "order"],
        bezier_derivative,
      ),
      ("bezier_tangent", &["bezier", "u"], bezier_tangent),
      ("bezier_curvature", &["bezier", "u"], bezier_curvature),
      (
        "bezier_length",
        &["bezier", "start_u", "end_u", "max_deflect"],
        bezier_length,
      ),
      (
        "bezier_closest_point",
        &["bezier", "pt", "max_err"],
        bezier_closest_point,
      ),
      (
        "bezier_line_intersection",
        &["bezier", "line"],
        bezier_line_intersection,
      ),
      (
        "bezpath_points",
        &["bezpath", "curveind", "u", "N"],
        bezpath_points,
      ),
      (
        "bezpath_curve",
        &["bezpath", "splinesteps", "N", "endpoint"],
        bezpath_curve,
      ),
      (
        "bezpath_length",
        &["bezpath", "N", "max_deflect"],
        bezpath_length,
      ),
      (
        "bezpath_closest_point",
        &["bezpath", "pt", "N", "max_err"],
        bezpath_closest_point,
      ),
      (
        "path_to_bezpath",
        &["path", "closed", "tangents", "uniform", "size", "relsize"],
        path_to_bezpath,
      ),
      (
        "bezpath_close_to_axis",
        &["bezpath", "axis", "N"],
        bezpath_close_to_axis,
      ),
      (
        "bezpath_offset",
        &["offset", "bezpath", "N"],
        bezpath_offset,
      ),
      ("is_bezier_patch", &["x", "dim"], is_bezier_patch),
      (
        "bezier_patch_points",
        &["patch", "u", "v"],
        bezier_patch_points,
      ),
      (
        "bezier_patch_normals",
        &["patch", "u", "v"],
        bezier_patch_normals,
      ),
      ("bezier_patch_reverse", &["patch"], bezier_patch_reverse),
      ("bezier_patch_flat", &["patch", "eps"], bezier_patch_flat),
      (
        "bezier_vnf",
        &["patches", "splinesteps", "style"],
        bezier_vnf,
      ),
      (
        "bezier_vnf_degenerate_patch",
        &["patch", "splinesteps", "reverse", "return_edges"],
        bezier_vnf_degenerate_patch,
      ),
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

  const QUARTER: &str = "{{10,0},{10,5.5228},{5.5228,10},{0,10}}";

  #[test]
  fn a_bezier_starts_and_ends_on_its_outer_control_points() {
    let p: Vec<f64> = eval(&format!("return bosl.bezier_points({QUARTER}, 0)"));
    assert!(close(&p, &[10.0, 0.0]), "{p:?}");
    let p: Vec<f64> = eval(&format!("return bosl.bezier_points({QUARTER}, 1)"));
    assert!(close(&p, &[0.0, 10.0]), "{p:?}");
  }

  #[test]
  fn a_cubic_approximation_of_a_quarter_circle_stays_on_the_radius() {
    let r: f64 = eval(&format!(
      "local p = bosl.bezier_points({QUARTER}, 0.5)
       return math.sqrt(p[1]*p[1] + p[2]*p[2])"
    ));
    assert!((r - 10.0).abs() < 0.01, "{r}");
  }

  #[test]
  fn a_curve_is_sampled_at_the_requested_resolution() {
    let n: usize = eval(&format!("return #bosl.bezier_curve({QUARTER}, 8)"));
    assert_eq!(n, 9);
    let n: usize =
      eval(&format!("return #bosl.bezier_curve({QUARTER}, 8, false)"));
    assert_eq!(n, 8);
  }

  #[test]
  fn a_straight_bezier_has_a_constant_tangent_and_no_curvature() {
    let line = "{{0,0},{10,0}}";
    let t: Vec<f64> = eval(&format!("return bosl.bezier_tangent({line}, 0.3)"));
    assert!(close(&t, &[1.0, 0.0]), "{t:?}");
    let k: f64 = eval(&format!("return bosl.bezier_curvature({line}, 0.5)"));
    assert!(k.abs() < 1e-9, "{k}");
  }

  #[test]
  fn curvature_of_a_circular_arc_is_one_over_its_radius() {
    let k: f64 = eval(&format!("return bosl.bezier_curvature({QUARTER}, 0.5)"));
    assert!((k - 0.1).abs() < 1e-3, "{k}");
  }

  #[test]
  fn the_length_of_a_quarter_circle_is_a_quarter_of_its_circumference() {
    let l: f64 = eval(&format!("return bosl.bezier_length({QUARTER})"));
    let ideal = std::f64::consts::PI * 10.0 / 2.0;
    assert!((l - ideal).abs() < 0.01, "{l} vs {ideal}");
  }

  #[test]
  fn a_straight_bezier_measures_its_own_length() {
    let l: f64 = eval("return bosl.bezier_length({{0,0},{3,4}})");
    assert!((l - 5.0).abs() < 1e-9, "{l}");
  }

  #[test]
  fn the_closest_point_is_found_anywhere_along_the_curve() {
    let u: f64 =
      eval("return bosl.bezier_closest_point({{0,0},{10,0}}, {3,5})");
    assert!((u - 0.3).abs() < 0.01, "{u}");
  }

  #[test]
  fn a_line_crossing_a_curve_is_found_at_the_right_parameter() {
    let us: Vec<f64> = eval(
      "return bosl.bezier_line_intersection({{0,0},{10,0}}, {{5,-5},{5,5}})",
    );
    assert_eq!(us.len(), 1);
    assert!((us[0] - 0.5).abs() < 1e-6, "{us:?}");
  }

  #[test]
  fn a_bezpath_runs_through_all_its_segments() {
    // Two cubic segments need seven points: four each, sharing the middle.
    let path = "{{0,0},{5,10},{10,10},{15,0},{20,-10},{25,0},{30,10}}";
    let n: usize = eval(&format!("return #bosl.bezpath_curve({path}, 4)"));
    // Two segments at four steps each, plus the final point.
    assert_eq!(n, 9);
    let p: Vec<f64> = eval(&format!("return bosl.bezpath_curve({path}, 4)[9]"));
    assert!(close(&p, &[30.0, 10.0]), "{p:?}");
  }

  #[test]
  fn a_bezpath_of_the_wrong_length_is_rejected() {
    let lua = Lua::new();
    register_bosl(&lua).unwrap();
    let err = lua
      .load("return bosl.bezpath_curve({{0,0},{1,1},{2,0}})")
      .eval::<mlua::Value>()
      .unwrap_err()
      .to_string();
    assert!(err.contains("multiple of 3"), "{err}");
  }

  #[test]
  fn a_bezpath_measures_the_sum_of_its_segments() {
    let l: f64 = eval(
      "return bosl.bezpath_length({{0,0},{1,0},{2,0},{3,0},{4,0},{5,0},{6,0}})",
    );
    assert!((l - 6.0).abs() < 1e-6, "{l}");
  }

  #[test]
  fn fitting_a_bezpath_passes_through_every_input_point() {
    let pts: Vec<Vec<f64>> = eval(
      "local bp = bosl.path_to_bezpath({{0,0},{10,10},{20,0}})
       return {bp[1], bp[4], bp[7]}",
    );
    assert!(close(&pts[0], &[0.0, 0.0]), "{pts:?}");
    assert!(close(&pts[1], &[10.0, 10.0]), "{pts:?}");
    assert!(close(&pts[2], &[20.0, 0.0]), "{pts:?}");
  }

  #[test]
  fn a_patch_interpolates_between_its_corners() {
    let patch = "{{{0,0,0},{10,0,0}},{{0,10,0},{10,10,0}}}";
    let p: Vec<f64> =
      eval(&format!("return bosl.bezier_patch_points({patch}, 0, 0)"));
    assert!(close(&p, &[0.0, 0.0, 0.0]), "{p:?}");
    let p: Vec<f64> =
      eval(&format!("return bosl.bezier_patch_points({patch}, 1, 1)"));
    assert!(close(&p, &[10.0, 10.0, 0.0]), "{p:?}");
    let p: Vec<f64> = eval(&format!(
      "return bosl.bezier_patch_points({patch}, 0.5, 0.5)"
    ));
    assert!(close(&p, &[5.0, 5.0, 0.0]), "{p:?}");
  }

  #[test]
  fn a_flat_patch_is_recognised_and_its_normal_points_up() {
    let patch = "{{{0,0,0},{10,0,0}},{{0,10,0},{10,10,0}}}";
    assert!(eval::<bool>(&format!(
      "return bosl.bezier_patch_flat({patch})"
    )));
    let n: Vec<f64> = eval(&format!(
      "return bosl.bezier_patch_normals({patch}, 0.5, 0.5)"
    ));
    assert!(close(&n, &[0.0, 0.0, 1.0]), "{n:?}");
  }

  #[test]
  fn a_patch_becomes_a_mesh_of_points_and_faces() {
    let counts: Vec<usize> = eval(
      "local v = bosl.bezier_vnf({{{0,0,0},{10,0,0}},{{0,10,0},{10,10,0}}}, 4)
       return {#v[1], #v[2]}",
    );
    // A 5x5 grid of points, and two triangles per cell.
    assert_eq!(counts[0], 25);
    assert_eq!(counts[1], 32);
  }

  #[test]
  fn patch_predicates_tell_a_patch_from_a_path() {
    assert!(eval::<bool>(
      "return bosl.is_bezier_patch({{{0,0,0},{1,0,0}},{{0,1,0},{1,1,0}}})"
    ));
    assert!(!eval::<bool>("return bosl.is_bezier_patch({{0,0},{1,1}})"));
  }
}
