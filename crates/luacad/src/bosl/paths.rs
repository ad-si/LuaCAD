//! BOSL2's `paths.scad`: measuring, resampling and cutting polylines.
//!
//! A path is a list of points. `closed` says whether the last point joins
//! back to the first; it defaults to false for open measurements and true
//! where BOSL2 treats the path as an outline.

use mlua::{Lua, Result as LuaResult, Value as LuaValue};

use crate::bosl::value::{Args, PureFn, Val, num_list, register_all, v3};

const EPS: f64 = 1e-9;

fn sub(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
  [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn norm(a: [f64; 3]) -> f64 {
  (a[0] * a[0] + a[1] * a[1] + a[2] * a[2]).sqrt()
}

fn unit(a: [f64; 3]) -> Option<[f64; 3]> {
  let n = norm(a);
  if n < EPS {
    None
  } else {
    Some([a[0] / n, a[1] / n, a[2] / n])
  }
}

/// The width of the points a path is made of, so 2D input stays 2D.
fn path_dim(a: &Args, name: &str) -> usize {
  a.val(name)
    .and_then(|v| v.as_matrix())
    .and_then(|m| m.first().map(|p| p.len()))
    .unwrap_or(3)
    .clamp(2, 3)
}

fn out_point(p: [f64; 3], dim: usize) -> Val {
  Val::vec(p[..dim].to_vec())
}

/// Read a path, accepting a single-outline region as the path it contains.
pub fn read_path(a: &Args, name: &str) -> LuaResult<Vec<[f64; 3]>> {
  let Some(v) = a.val(name) else {
    return a.err(format!("{name} is required"));
  };
  if let Some(m) = v.as_matrix() {
    return Ok(m.iter().map(|p| v3(p)).collect());
  }
  // A region holding exactly one outline stands in for that outline.
  if let Some(items) = v.as_list()
    && items.len() == 1
    && let Some(m) = items[0].as_matrix()
  {
    return Ok(m.iter().map(|p| v3(p)).collect());
  }
  a.err(format!("{name} must be a path"))
}

/// The points of a path in walking order, with the closing point appended
/// when the path is closed.
fn walk(path: &[[f64; 3]], closed: bool) -> Vec<[f64; 3]> {
  if closed && !path.is_empty() {
    path
      .iter()
      .copied()
      .chain(std::iter::once(path[0]))
      .collect()
  } else {
    path.to_vec()
  }
}

/// The running distance to each point along the path.
fn cumulative(path: &[[f64; 3]], closed: bool) -> Vec<f64> {
  let pts = walk(path, closed);
  let mut out = vec![0.0];
  for w in pts.windows(2) {
    out.push(out[out.len() - 1] + norm(sub(w[1], w[0])));
  }
  out
}

// ---------------------------------------------------------------------------
// Predicates and coercion
// ---------------------------------------------------------------------------

fn is_path(_lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let ok = match a.val("list").and_then(|v| v.as_matrix()) {
    Some(m) => {
      let width = m.first().map(|p| p.len()).unwrap_or(0);
      let rectangular = m.iter().all(|p| p.len() == width);
      let dims: Vec<usize> = match a.val("dim") {
        Some(Val::Num(n)) => vec![n as usize],
        Some(other) => other
          .as_vec()
          .map(|v| v.iter().map(|n| *n as usize).collect())
          .unwrap_or_else(|| vec![2, 3]),
        None => vec![2, 3],
      };
      m.len() > 1 && width > 0 && rectangular && dims.contains(&width)
    }
    None => false,
  };
  Ok(LuaValue::Boolean(ok))
}

/// Whether the value is a region holding exactly one outline.
fn is_1region(_lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let ok = a
    .val("path")
    .and_then(|v| v.as_list().map(|s| s.to_vec()))
    .is_some_and(|items| {
      items.len() == 1 && items[0].as_matrix().is_some_and(|m| m.len() > 1)
    });
  Ok(LuaValue::Boolean(ok))
}

fn force_path(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let path = read_path(a, "path")?;
  let dim = path_dim(a, "path").max(2);
  Val::list(path.iter().map(|p| out_point(*p, dim))).to_lua(lua)
}

/// Wrap a bare path into a region, which is a list of outlines.
fn force_region(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let Some(v) = a.val("path") else {
    return a.err("path is required");
  };
  if v.as_matrix().is_some() {
    return Val::List(vec![v]).to_lua(lua);
  }
  v.to_lua(lua)
}

// ---------------------------------------------------------------------------
// Measurement
// ---------------------------------------------------------------------------

fn path_length(_lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let path = read_path(a, "path")?;
  let closed = a.bool_or("closed", false);
  Ok(LuaValue::Number(
    *cumulative(&path, closed).last().unwrap_or(&0.0),
  ))
}

fn path_segment_lengths(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let path = read_path(a, "path")?;
  let closed = a.bool_or("closed", false);
  let pts = walk(&path, closed);
  let lens: Vec<f64> = pts.windows(2).map(|w| norm(sub(w[1], w[0]))).collect();
  num_list(lua, &lens)
}

fn path_closest_point(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let path = read_path(a, "path")?;
  let dim = path_dim(a, "path");
  let pt = v3(&a.need_vec("pt")?);
  let closed = a.bool_or("closed", true);
  let pts = walk(&path, closed);
  if pts.len() < 2 {
    return a.err("the path needs at least two points");
  }

  let mut best = (0usize, pts[0], f64::INFINITY);
  for (i, w) in pts.windows(2).enumerate() {
    let d = sub(w[1], w[0]);
    let len2 = d[0] * d[0] + d[1] * d[1] + d[2] * d[2];
    let t = if len2 < EPS {
      0.0
    } else {
      (((pt[0] - w[0][0]) * d[0]
        + (pt[1] - w[0][1]) * d[1]
        + (pt[2] - w[0][2]) * d[2])
        / len2)
        .clamp(0.0, 1.0)
    };
    let q = [w[0][0] + d[0] * t, w[0][1] + d[1] * t, w[0][2] + d[2] * t];
    let gap = norm(sub(q, pt));
    if gap < best.2 {
      best = (i, q, gap);
    }
  }
  // The answer names the segment as well as the point on it.
  Val::list([Val::Num(best.0 as f64), out_point(best.1, dim)]).to_lua(lua)
}

/// The unit tangent at each point of a path.
fn path_tangents(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let path = read_path(a, "path")?;
  let dim = path_dim(a, "path");
  let closed = a.bool_or("closed", false);
  let n = path.len();
  if n < 2 {
    return a.err("the path needs at least two points");
  }
  let tangents: Vec<Val> = (0..n)
    .map(|i| {
      // Interior points use the direction between their neighbours; the
      // ends of an open path use the one segment they have.
      let d = if closed {
        sub(path[(i + 1) % n], path[(i + n - 1) % n])
      } else if i == 0 {
        sub(path[1], path[0])
      } else if i == n - 1 {
        sub(path[n - 1], path[n - 2])
      } else {
        sub(path[i + 1], path[i - 1])
      };
      out_point(unit(d).unwrap_or([0.0; 3]), dim)
    })
    .collect();
  Val::List(tangents).to_lua(lua)
}

/// The unit normal at each point of a path.
fn path_normals(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let path = read_path(a, "path")?;
  let dim = path_dim(a, "path");
  let closed = a.bool_or("closed", false);
  let n = path.len();
  if n < 2 {
    return a.err("the path needs at least two points");
  }
  let normals: Vec<Val> = (0..n)
    .map(|i| {
      let d = if closed {
        sub(path[(i + 1) % n], path[(i + n - 1) % n])
      } else if i == 0 {
        sub(path[1], path[0])
      } else if i == n - 1 {
        sub(path[n - 1], path[n - 2])
      } else {
        sub(path[i + 1], path[i - 1])
      };
      let t = unit(d).unwrap_or([1.0, 0.0, 0.0]);
      if dim == 2 {
        // In the plane the normal is the tangent turned a quarter turn.
        out_point([t[1], -t[0], 0.0], 2)
      } else {
        // In space it is the direction the path curves towards.
        let prev = path[(i + n - 1) % n];
        let next = path[(i + 1) % n];
        let curve = sub(sub(next, path[i]), sub(path[i], prev));
        let perp = [
          curve[0]
            - t[0] * (curve[0] * t[0] + curve[1] * t[1] + curve[2] * t[2]),
          curve[1]
            - t[1] * (curve[0] * t[0] + curve[1] * t[1] + curve[2] * t[2]),
          curve[2]
            - t[2] * (curve[0] * t[0] + curve[1] * t[1] + curve[2] * t[2]),
        ];
        out_point(
          unit(perp).unwrap_or_else(|| {
            // A straight run has no curvature to point at, so any
            // perpendicular will do.
            unit([-t[1], t[0], 0.0]).unwrap_or([0.0, 0.0, 1.0])
          }),
          3,
        )
      }
    })
    .collect();
  Val::List(normals).to_lua(lua)
}

// ---------------------------------------------------------------------------
// Resampling
// ---------------------------------------------------------------------------

/// The point a given distance along the path.
fn point_at(path: &[[f64; 3]], cum: &[f64], d: f64) -> [f64; 3] {
  let pts = if cum.len() > path.len() {
    walk(path, true)
  } else {
    path.to_vec()
  };
  let total = *cum.last().unwrap_or(&0.0);
  let d = d.clamp(0.0, total);
  let i = cum
    .iter()
    .rposition(|c| *c <= d + 1e-12)
    .unwrap_or(0)
    .min(pts.len().saturating_sub(2));
  let seg = cum[i + 1] - cum[i];
  let t = if seg < 1e-12 { 0.0 } else { (d - cum[i]) / seg };
  [
    pts[i][0] + (pts[i + 1][0] - pts[i][0]) * t,
    pts[i][1] + (pts[i + 1][1] - pts[i][1]) * t,
    pts[i][2] + (pts[i + 1][2] - pts[i][2]) * t,
  ]
}

fn resample_path(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let path = read_path(a, "path")?;
  let dim = path_dim(a, "path");
  let closed = a.bool_or("closed", true);
  let cum = cumulative(&path, closed);
  let total = *cum.last().unwrap_or(&0.0);
  if total < EPS {
    return a.err("the path has no length to resample");
  }

  let count = match (a.int("n"), a.num("spacing")) {
    (Some(n), None) if n > 0 => n as usize,
    // An open path needs one more point than it has gaps; a closed one
    // wraps, so its gaps and points come to the same number.
    (None, Some(s)) if s > 0.0 => {
      let gaps = (total / s).round().max(1.0) as usize;
      if closed { gaps } else { gaps + 1 }
    }
    _ => return a.err("give exactly one of n and spacing"),
  };
  // A closed path has no separate last point; an open one keeps both ends.
  let step = if closed {
    total / count as f64
  } else if count > 1 {
    total / (count - 1) as f64
  } else {
    0.0
  };
  let pts: Vec<Val> = (0..count)
    .map(|i| out_point(point_at(&path, &cum, i as f64 * step), dim))
    .collect();
  Val::List(pts).to_lua(lua)
}

fn subdivide_path(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let path = read_path(a, "path")?;
  let dim = path_dim(a, "path");
  let closed = a.bool_or("closed", true);
  let pts = walk(&path, closed);
  if pts.len() < 2 {
    return a.err("the path needs at least two points");
  }
  let segments = pts.len() - 1;

  // Each segment is cut into this many pieces.
  let per_segment: Vec<usize> =
    match (a.int("n"), a.int("refine"), a.num("maxlen")) {
      (_, _, Some(maxlen)) if maxlen > 0.0 => pts
        .windows(2)
        .map(|w| (norm(sub(w[1], w[0])) / maxlen).ceil().max(1.0) as usize)
        .collect(),
      (_, Some(refine), _) if refine > 0 => {
        vec![refine as usize; segments]
      }
      (Some(n), _, _) if n > 0 => {
        // Spread the extra points over the segments in proportion to their
        // length, so the result is evenly spaced rather than evenly divided.
        let want = n as usize;
        let lengths: Vec<f64> =
          pts.windows(2).map(|w| norm(sub(w[1], w[0]))).collect();
        let total: f64 = lengths.iter().sum();
        let mut counts: Vec<usize> = lengths
          .iter()
          .map(|l| ((l / total) * want as f64).round().max(1.0) as usize)
          .collect();
        // Rounding may overshoot or undershoot the requested total.
        let mut have: usize = counts.iter().sum();
        while have > want && counts.iter().any(|c| *c > 1) {
          let i = counts
            .iter()
            .enumerate()
            .filter(|(_, c)| **c > 1)
            .max_by(|a, b| lengths[a.0].total_cmp(&lengths[b.0]))
            .map(|(i, _)| i)
            .unwrap_or(0);
          counts[i] -= 1;
          have -= 1;
        }
        while have < want {
          let i = counts
            .iter()
            .enumerate()
            .max_by(|a, b| {
              (lengths[a.0] / *a.1 as f64)
                .total_cmp(&(lengths[b.0] / *b.1 as f64))
            })
            .map(|(i, _)| i)
            .unwrap_or(0);
          counts[i] += 1;
          have += 1;
        }
        counts
      }
      _ => return a.err("give exactly one of n, refine and maxlen"),
    };

  let mut out: Vec<Val> = Vec::new();
  for (w, steps) in pts.windows(2).zip(per_segment.iter()) {
    for k in 0..*steps {
      let t = k as f64 / *steps as f64;
      out.push(out_point(
        [
          w[0][0] + (w[1][0] - w[0][0]) * t,
          w[0][1] + (w[1][1] - w[0][1]) * t,
          w[0][2] + (w[1][2] - w[0][2]) * t,
        ],
        dim,
      ));
    }
  }
  if !closed {
    out.push(out_point(pts[pts.len() - 1], dim));
  }
  Val::List(out).to_lua(lua)
}

fn path_merge_collinear(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let path = read_path(a, "path")?;
  let dim = path_dim(a, "path");
  let closed = a.bool_or("closed", false);
  let eps = a.num_or("eps", EPS);
  if path.len() <= 2 {
    return Val::list(path.iter().map(|p| out_point(*p, dim))).to_lua(lua);
  }

  // Drop repeated points first, then any vertex whose neighbours run
  // straight through it.
  let mut deduped: Vec<[f64; 3]> = Vec::with_capacity(path.len());
  for p in &path {
    if deduped.last().is_none_or(|q| norm(sub(*p, *q)) > eps) {
      deduped.push(*p);
    }
  }
  if closed
    && deduped.len() > 1
    && norm(sub(deduped[0], deduped[deduped.len() - 1])) <= eps
  {
    deduped.pop();
  }

  let n = deduped.len();
  let mut out: Vec<Val> = Vec::new();
  if !closed {
    out.push(out_point(deduped[0], dim));
  }
  let range: Vec<usize> = if closed {
    (0..n).collect()
  } else {
    (1..n - 1).collect()
  };
  for i in range {
    let prev = deduped[(i + n - 1) % n];
    let here = deduped[i];
    let next = deduped[(i + 1) % n];
    let d1 = sub(here, prev);
    let d2 = sub(next, here);
    let cross = [
      d1[1] * d2[2] - d1[2] * d2[1],
      d1[2] * d2[0] - d1[0] * d2[2],
      d1[0] * d2[1] - d1[1] * d2[0],
    ];
    // Compare the turn against the segment lengths, so the tolerance means
    // the same thing on a large path as on a small one.
    let scale = norm(d1).max(norm(d2)).max(EPS);
    if norm(cross) / scale > eps {
      out.push(out_point(here, dim));
    }
  }
  if !closed {
    out.push(out_point(deduped[n - 1], dim));
  }
  Val::List(out).to_lua(lua)
}

// ---------------------------------------------------------------------------
// Cutting
// ---------------------------------------------------------------------------

fn path_cut_points(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let path = read_path(a, "path")?;
  let dim = path_dim(a, "path");
  let closed = a.bool_or("closed", false);
  let cum = cumulative(&path, closed);
  let single = matches!(a.val("cutdist"), Some(Val::Num(_)));
  let dists: Vec<f64> = match a.val("cutdist") {
    Some(Val::Num(d)) => vec![d],
    Some(other) => match other.as_vec() {
      Some(v) => v,
      None => return a.err("cutdist must be a distance or a list of them"),
    },
    None => return a.err("cutdist is required"),
  };

  let entry = |d: f64| -> Val {
    let p = point_at(&path, &cum, d);
    // Each cut names the point, the segment it falls in, and how far along
    // the whole path it is.
    let seg = cum
      .iter()
      .rposition(|c| *c <= d + 1e-12)
      .unwrap_or(0)
      .min(path.len().saturating_sub(1));
    Val::list([out_point(p, dim), Val::Num(seg as f64), Val::Num(d)])
  };

  if single {
    return entry(dists[0]).to_lua(lua);
  }
  Val::list(dists.iter().map(|d| entry(*d))).to_lua(lua)
}

fn path_cut(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let path = read_path(a, "path")?;
  let dim = path_dim(a, "path");
  let closed = a.bool_or("closed", false);
  let cum = cumulative(&path, closed);
  let total = *cum.last().unwrap_or(&0.0);
  let mut dists: Vec<f64> = match a.val("cutdist") {
    Some(Val::Num(d)) => vec![d],
    Some(other) => match other.as_vec() {
      Some(v) => v,
      None => return a.err("cutdist must be a distance or a list of them"),
    },
    None => return a.err("cutdist is required"),
  };
  dists.sort_by(f64::total_cmp);
  if dists.iter().any(|d| *d <= EPS || *d >= total - EPS) {
    return a.err("the cut distances must lie strictly inside the path");
  }

  // Walk the path once, starting a new piece at each cut.
  let pts = walk(&path, closed);
  let mut pieces: Vec<Vec<Val>> = Vec::new();
  let mut current: Vec<Val> = vec![out_point(pts[0], dim)];
  let mut next_cut = 0usize;
  for i in 1..pts.len() {
    while next_cut < dists.len() && dists[next_cut] < cum[i] - 1e-12 {
      let p = point_at(&path, &cum, dists[next_cut]);
      current.push(out_point(p, dim));
      pieces.push(std::mem::take(&mut current));
      current.push(out_point(p, dim));
      next_cut += 1;
    }
    current.push(out_point(pts[i], dim));
  }
  pieces.push(current);
  Val::list(pieces.into_iter().map(Val::List)).to_lua(lua)
}

pub fn register(lua: &Lua, bosl: &mlua::Table) -> LuaResult<()> {
  register_all(
    lua,
    bosl,
    &[
      ("is_path", &["list", "dim", "fast"], is_path as PureFn),
      ("is_1region", &["path", "name"], is_1region),
      ("force_path", &["path", "name"], force_path),
      ("force_region", &["path"], force_region),
      ("path_length", &["path", "closed"], path_length),
      (
        "path_segment_lengths",
        &["path", "closed"],
        path_segment_lengths,
      ),
      (
        "path_closest_point",
        &["path", "pt", "closed"],
        path_closest_point,
      ),
      (
        "path_tangents",
        &["path", "closed", "uniform"],
        path_tangents,
      ),
      // LuaCAD registered the singular spellings before these were ported;
      // both names reach the same function.
      (
        "path_tangent",
        &["path", "closed", "uniform"],
        path_tangents,
      ),
      (
        "path_normals",
        &["path", "tangents", "closed"],
        path_normals,
      ),
      ("path_normal", &["path", "tangents", "closed"], path_normals),
      (
        "resample_path",
        &["path", "n", "spacing", "keep_corners", "closed"],
        resample_path,
      ),
      (
        "subdivide_path",
        &["path", "n", "refine", "maxlen", "closed", "exact", "method"],
        subdivide_path,
      ),
      (
        "path_merge_collinear",
        &["path", "closed", "eps"],
        path_merge_collinear,
      ),
      (
        "path_cut_points",
        &["path", "cutdist", "closed", "direction"],
        path_cut_points,
      ),
      ("path_cut", &["path", "cutdist", "closed"], path_cut),
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

  #[test]
  fn path_length_adds_up_the_segments() {
    let l: f64 = eval("return bosl.path_length({{0,0},{3,0},{3,4}})");
    assert_eq!(l, 7.0);
    // Closing the path adds the run back to the start.
    let l: f64 = eval("return bosl.path_length({{0,0},{3,0},{3,4}}, true)");
    assert_eq!(l, 12.0);
  }

  #[test]
  fn segment_lengths_are_reported_one_per_span() {
    let l: Vec<f64> =
      eval("return bosl.path_segment_lengths({{0,0},{3,0},{3,4}})");
    assert_eq!(l, vec![3.0, 4.0]);
    let l: Vec<f64> =
      eval("return bosl.path_segment_lengths({{0,0},{3,0},{3,4}}, true)");
    assert_eq!(l, vec![3.0, 4.0, 5.0]);
  }

  #[test]
  fn the_closest_point_names_its_segment_too() {
    let seg: f64 = eval(
      "return bosl.path_closest_point({{0,0},{10,0},{10,10}}, {5,3}, false)[1]",
    );
    assert_eq!(seg, 0.0);
    let p: Vec<f64> = eval(
      "return bosl.path_closest_point({{0,0},{10,0},{10,10}}, {5,3}, false)[2]",
    );
    assert_eq!(p, vec![5.0, 0.0]);
  }

  #[test]
  fn tangents_and_normals_come_back_in_the_paths_own_dimension() {
    let t: Vec<Vec<f64>> =
      eval("return bosl.path_tangents({{0,0},{10,0},{10,10}})");
    assert_eq!(t.len(), 3);
    assert_eq!(t[0], vec![1.0, 0.0]);
    assert_eq!(t[2], vec![0.0, 1.0]);
    let n: Vec<Vec<f64>> =
      eval("return bosl.path_normals({{0,0},{10,0},{10,10}})");
    assert_eq!(n[0], vec![0.0, -1.0]);
  }

  #[test]
  fn resampling_spaces_points_evenly_along_the_path() {
    let p: Vec<Vec<f64>> =
      eval("return bosl.resample_path({{0,0},{10,0}}, 3, nil, nil, false)");
    assert_eq!(p.len(), 3);
    assert_eq!(p[0], vec![0.0, 0.0]);
    assert_eq!(p[1], vec![5.0, 0.0]);
    assert_eq!(p[2], vec![10.0, 0.0]);
  }

  #[test]
  fn resampling_by_spacing_picks_its_own_count() {
    let n: usize =
      eval("return #bosl.resample_path({{0,0},{100,0}}, nil, 10, nil, false)");
    assert_eq!(n, 11);
  }

  #[test]
  fn subdividing_adds_points_without_moving_the_path() {
    let p: Vec<Vec<f64>> = eval(
      "return bosl.subdivide_path({{0,0},{10,0}}, {refine = 2, closed = false})",
    );
    assert_eq!(p, vec![vec![0.0, 0.0], vec![5.0, 0.0], vec![10.0, 0.0]]);
  }

  #[test]
  fn subdividing_by_maximum_length_bounds_every_segment() {
    let lens: Vec<f64> = eval(
      "local p = bosl.subdivide_path({{0,0},{25,0}}, {maxlen = 10, closed = false})
       return bosl.path_segment_lengths(p)",
    );
    assert!(lens.iter().all(|l| *l <= 10.0 + 1e-9), "{lens:?}");
  }

  #[test]
  fn collinear_points_are_merged_away() {
    let p: Vec<Vec<f64>> =
      eval("return bosl.path_merge_collinear({{0,0},{5,0},{10,0},{10,10}})");
    assert_eq!(p, vec![vec![0.0, 0.0], vec![10.0, 0.0], vec![10.0, 10.0]]);
  }

  #[test]
  fn cutting_a_path_splits_it_at_the_given_distances() {
    let pieces: Vec<Vec<Vec<f64>>> =
      eval("return bosl.path_cut({{0,0},{10,0}}, {4})");
    assert_eq!(pieces.len(), 2);
    assert_eq!(pieces[0].last().unwrap(), &vec![4.0, 0.0]);
    assert_eq!(pieces[1][0], vec![4.0, 0.0]);
  }

  #[test]
  fn a_cut_point_reports_where_along_the_path_it_falls() {
    let d: f64 = eval("return bosl.path_cut_points({{0,0},{10,0}}, 4)[3]");
    assert_eq!(d, 4.0);
    let p: Vec<f64> = eval("return bosl.path_cut_points({{0,0},{10,0}}, 4)[1]");
    assert_eq!(p, vec![4.0, 0.0]);
  }

  #[test]
  fn path_predicates_recognise_paths_and_regions() {
    assert!(eval::<bool>("return bosl.is_path({{0,0},{1,1}})"));
    assert!(!eval::<bool>("return bosl.is_path({{0,0}})"));
    assert!(!eval::<bool>("return bosl.is_path({1,2,3})"));
    assert!(eval::<bool>("return bosl.is_1region({{{0,0},{1,1}}})"));
    assert!(!eval::<bool>("return bosl.is_1region({{0,0},{1,1}})"));
  }

  #[test]
  fn a_single_outline_region_is_accepted_wherever_a_path_is() {
    let l: f64 = eval("return bosl.path_length({{{0,0},{3,0},{3,4}}})");
    assert_eq!(l, 7.0);
  }
}
