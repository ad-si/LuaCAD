//! BOSL2's `regions.scad`: areas of the plane bounded by several outlines.
//!
//! A region is a list of closed 2D outlines. An outline drawn inside another
//! is a hole, however either of them winds, so a washer is two circles and a
//! letter O is the same. Anything that has to decide what is inside counts
//! crossings: a point enclosed by an odd number of outlines is in the region.
//!
//! A region is *valid* when its outlines neither cross themselves nor each
//! other. Most of what follows assumes that, and [`make_region`] is how a
//! pile of arbitrary polygons becomes one.

use mlua::{Lua, Result as LuaResult, Value as LuaValue};

use crate::bosl::value::{Args, PureFn, Val, register_all};
use crate::export::{AreaOp, combine_outlines, combine_outlines_with_rule};

const EPS: f64 = 1e-9;

// ---------------------------------------------------------------------------
// Reading and writing regions
// ---------------------------------------------------------------------------

/// Read a region: a list of closed 2D outlines.
///
/// A bare outline counts as a region of one, which is what `force_region`
/// does, so callers can pass either.
pub fn read_region(a: &Args, name: &str) -> LuaResult<Vec<Vec<[f64; 2]>>> {
  let Some(v) = a.val(name) else {
    return a.err(format!("{name} is required"));
  };
  match region_of(&v) {
    Some(r) => Ok(r),
    None => a.err(format!("{name} must be a region: a list of 2D outlines")),
  }
}

/// Interpret a value as a region, accepting a single outline as a region of
/// one. `None` when it is neither.
fn region_of(v: &Val) -> Option<Vec<Vec<[f64; 2]>>> {
  let items = v.as_list()?;
  if items.is_empty() {
    return Some(Vec::new());
  }
  // A list of points is one outline; a list of those is a region.
  if let Some(path) = as_path(&items[0]) {
    let _ = path;
    let mut out = Vec::with_capacity(items.len());
    for item in items {
      out.push(as_path(item)?);
    }
    return Some(out);
  }
  // A bare outline: its entries are points, not paths.
  let path = v.as_matrix()?;
  if path.iter().any(|p| p.len() < 2) {
    return None;
  }
  Some(vec![path.iter().map(|p| [p[0], p[1]]).collect()])
}

/// A single closed outline, if that is what this value is.
fn as_path(v: &Val) -> Option<Vec<[f64; 2]>> {
  let rows = v.as_matrix()?;
  if rows.len() < 2 || rows.iter().any(|p| p.len() < 2) {
    return None;
  }
  Some(rows.iter().map(|p| [p[0], p[1]]).collect())
}

fn write_region(lua: &Lua, region: &[Vec<[f64; 2]>]) -> LuaResult<LuaValue> {
  Val::list(
    region
      .iter()
      .map(|path| Val::list(path.iter().map(|p| Val::vec(*p)))),
  )
  .to_lua(lua)
}

// ---------------------------------------------------------------------------
// Geometry helpers
// ---------------------------------------------------------------------------

fn cross2(a: [f64; 2], b: [f64; 2]) -> f64 {
  a[0] * b[1] - a[1] * b[0]
}

fn sub2(a: [f64; 2], b: [f64; 2]) -> [f64; 2] {
  [a[0] - b[0], a[1] - b[1]]
}

pub fn signed_area(path: &[[f64; 2]]) -> f64 {
  let n = path.len();
  (0..n)
    .map(|i| cross2(path[i], path[(i + 1) % n]))
    .sum::<f64>()
    / 2.0
}

/// Where a point sits relative to a closed outline: inside, on it, or out.
///
/// Reported the way BOSL2 does — `1` inside, `0` on the boundary, `-1`
/// outside — by counting how often a ray from the point crosses the outline.
pub fn point_in_polygon(p: [f64; 2], poly: &[[f64; 2]], eps: f64) -> i32 {
  let n = poly.len();
  // On the boundary wins over everything else, so it is checked first.
  for i in 0..n {
    let a = poly[i];
    let b = poly[(i + 1) % n];
    let ab = sub2(b, a);
    let ap = sub2(p, a);
    let len2 = ab[0] * ab[0] + ab[1] * ab[1];
    if len2 < eps * eps {
      if ap[0].abs() < eps && ap[1].abs() < eps {
        return 0;
      }
      continue;
    }
    if cross2(ap, ab).abs() <= eps * len2.sqrt() {
      let t = (ap[0] * ab[0] + ap[1] * ab[1]) / len2;
      if t >= -eps && t <= 1.0 + eps {
        return 0;
      }
    }
  }
  let mut inside = false;
  for i in 0..n {
    let a = poly[i];
    let b = poly[(i + 1) % n];
    if (a[1] > p[1]) != (b[1] > p[1]) {
      let x = a[0] + (p[1] - a[1]) / (b[1] - a[1]) * (b[0] - a[0]);
      if x > p[0] {
        inside = !inside;
      }
    }
  }
  if inside { 1 } else { -1 }
}

/// Where two segments meet, as the parameters along each, if they do.
fn segment_intersection(
  p1: [f64; 2],
  p2: [f64; 2],
  q1: [f64; 2],
  q2: [f64; 2],
) -> Option<(f64, f64)> {
  let r = sub2(p2, p1);
  let s = sub2(q2, q1);
  let denom = cross2(r, s);
  if denom.abs() < EPS {
    return None;
  }
  let qp = sub2(q1, p1);
  Some((cross2(qp, s) / denom, cross2(qp, r) / denom))
}

/// Whether a path ever crosses or touches itself.
///
/// Neighbouring segments share an endpoint, and on a closed path the first
/// and last do too, so those meetings are expected and not counted.
pub fn is_path_simple(path: &[[f64; 2]], closed: bool, eps: f64) -> bool {
  let n = path.len();
  let segs = if closed { n } else { n - 1 };
  if segs < 2 {
    return true;
  }
  for i in 0..segs {
    let a1 = path[i];
    let a2 = path[(i + 1) % n];
    for j in (i + 1)..segs {
      let adjacent = j == i + 1 || (closed && i == 0 && j == segs - 1);
      let b1 = path[j];
      let b2 = path[(j + 1) % n];
      match segment_intersection(a1, a2, b1, b2) {
        Some((t, u)) => {
          let on_a = t > eps && t < 1.0 - eps;
          let on_b = u > eps && u < 1.0 - eps;
          if adjacent {
            // Sharing an endpoint is fine; crossing anywhere else is not.
            if on_a && on_b {
              return false;
            }
          } else if t >= -eps && t <= 1.0 + eps && u >= -eps && u <= 1.0 + eps {
            return false;
          }
        }
        None => {
          // Parallel: overlapping collinear segments are a self-touch.
          if !adjacent && collinear_overlap(a1, a2, b1, b2, eps) {
            return false;
          }
        }
      }
    }
  }
  true
}

fn collinear_overlap(
  a1: [f64; 2],
  a2: [f64; 2],
  b1: [f64; 2],
  b2: [f64; 2],
  eps: f64,
) -> bool {
  let r = sub2(a2, a1);
  let len = (r[0] * r[0] + r[1] * r[1]).sqrt();
  if len < eps {
    return false;
  }
  if cross2(sub2(b1, a1), r).abs() > eps * len {
    return false;
  }
  let along =
    |p: [f64; 2]| (sub2(p, a1)[0] * r[0] + sub2(p, a1)[1] * r[1]) / (len * len);
  let (lo, hi) = {
    let (u, v) = (along(b1), along(b2));
    if u <= v { (u, v) } else { (v, u) }
  };
  hi > eps && lo < 1.0 - eps
}

/// Whether two outlines meet at all.
fn paths_intersect(a: &[[f64; 2]], b: &[[f64; 2]], eps: f64) -> bool {
  for i in 0..a.len() {
    let a1 = a[i];
    let a2 = a[(i + 1) % a.len()];
    for j in 0..b.len() {
      let b1 = b[j];
      let b2 = b[(j + 1) % b.len()];
      if let Some((t, u)) = segment_intersection(a1, a2, b1, b2)
        && t >= -eps
        && t <= 1.0 + eps
        && u >= -eps
        && u <= 1.0 + eps
      {
        return true;
      }
    }
  }
  false
}

fn to_winding(path: &[[f64; 2]], clockwise: bool) -> Vec<[f64; 2]> {
  let is_cw = signed_area(path) < 0.0;
  if is_cw == clockwise {
    path.to_vec()
  } else {
    path.iter().rev().copied().collect()
  }
}

// ---------------------------------------------------------------------------
// The region functions
// ---------------------------------------------------------------------------

fn is_region(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let ok = a
    .val("x")
    .and_then(|v| {
      let items = v.as_list()?.to_vec();
      Some(!items.is_empty() && as_path(&items[0]).is_some())
    })
    .unwrap_or(false);
  let _ = lua;
  Ok(LuaValue::Boolean(ok))
}

fn is_valid_region(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let region = read_region(a, "region")?;
  let eps = a.num_or("eps", EPS);
  let _ = lua;
  if region.iter().any(|p| p.len() < 3) {
    return Ok(LuaValue::Boolean(false));
  }
  if region.iter().any(|p| !is_path_simple(p, true, eps)) {
    return Ok(LuaValue::Boolean(false));
  }
  // No outline may cross another, and none may touch another's edge.
  for i in 0..region.len() {
    for j in (i + 1)..region.len() {
      if paths_intersect(&region[i], &region[j], eps) {
        return Ok(LuaValue::Boolean(false));
      }
    }
  }
  Ok(LuaValue::Boolean(true))
}

fn is_region_simple(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let region = read_region(a, "region")?;
  let eps = a.num_or("eps", EPS);
  let _ = lua;
  if region.iter().any(|p| !is_path_simple(p, true, eps)) {
    return Ok(LuaValue::Boolean(false));
  }
  for i in 0..region.len() {
    for j in (i + 1)..region.len() {
      // Only crossings disqualify a region here; one outline nested inside
      // another is an ordinary hole and stays simple.
      if paths_intersect(&region[i], &region[j], eps) {
        return Ok(LuaValue::Boolean(false));
      }
    }
  }
  Ok(LuaValue::Boolean(true))
}

/// Resolve a pile of polygons into one well-defined area.
///
/// Each polygon is first untangled from itself — `nonzero` decides whether
/// the two lobes of a figure-eight both count as solid — and the results are
/// then combined by exclusive-or, so an outline drawn inside another is a
/// hole. That is exactly how BOSL2 builds a region, and why `nonzero` makes
/// no difference to polygons that do not cross themselves.
fn make_region(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let polys = read_region(a, "polys")?;
  let nonzero = a.bool_or("nonzero", false);
  let out = polys
    .iter()
    .map(|p| {
      combine_outlines_with_rule(
        std::slice::from_ref(p),
        &[],
        AreaOp::Union,
        nonzero,
      )
    })
    .reduce(|acc, r| combine_outlines(&acc, &r, AreaOp::ExclusiveOr))
    .unwrap_or_default();
  write_region(lua, &out)
}

fn region_fn(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let region = read_region(a, "r")?;
  write_region(lua, &region)
}

fn region_area(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let region = read_region(a, "region")?;
  let _ = lua;
  // Each part contributes its outer boundary less its holes, which is what
  // summing the signed areas with a consistent winding comes to.
  let parts = split_into_parts(&region);
  let area: f64 = parts.iter().flatten().map(|p| -signed_area(p)).sum();
  Ok(LuaValue::Number(area))
}

/// Group a region's outlines into parts: each an outer boundary followed by
/// the holes immediately inside it.
fn split_into_parts(region: &[Vec<[f64; 2]>]) -> Vec<Vec<Vec<[f64; 2]>>> {
  let n = region.len();
  // How deeply each outline is nested inside the others.
  let inside: Vec<Vec<bool>> = (0..n)
    .map(|i| {
      let probe = [
        (region[i][0][0] + region[i][1][0]) / 2.0,
        (region[i][0][1] + region[i][1][1]) / 2.0,
      ];
      (0..n)
        .map(|j| i != j && point_in_polygon(probe, &region[j], EPS) >= 0)
        .collect()
    })
    .collect();
  let level: Vec<usize> = (0..n)
    .map(|i| inside[i].iter().filter(|b| **b).count())
    .collect();

  (0..n)
    .filter(|i| level[*i].is_multiple_of(2))
    .map(|i| {
      let mut part = vec![to_winding(&region[i], true)];
      for j in 0..n {
        if level[j] == level[i] + 1 && inside[j][i] {
          part.push(to_winding(&region[j], false));
        }
      }
      part
    })
    .collect()
}

fn region_parts(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let region = read_region(a, "region")?;
  let parts = split_into_parts(&region);
  Val::list(parts.iter().map(|part| {
    Val::list(
      part
        .iter()
        .map(|path| Val::list(path.iter().map(|p| Val::vec(*p)))),
    )
  }))
  .to_lua(lua)
}

fn are_regions_equal(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let r1 = read_region(a, "region1")?;
  let r2 = read_region(a, "region2")?;
  let either = a.bool_or("either_winding", false);
  let _ = lua;
  if r1.len() != r2.len() {
    return Ok(LuaValue::Boolean(false));
  }
  let prep = |r: &[Vec<[f64; 2]>]| -> Vec<Vec<[f64; 2]>> {
    if either {
      r.iter().map(|p| to_winding(p, true)).collect()
    } else {
      r.to_vec()
    }
  };
  let (r1, r2) = (prep(&r1), prep(&r2));
  let mut taken = vec![false; r2.len()];
  for p in &r1 {
    match r2
      .iter()
      .enumerate()
      .position(|(i, q)| !taken[i] && same_polygon(p, q))
    {
      Some(i) => taken[i] = true,
      None => return Ok(LuaValue::Boolean(false)),
    }
  }
  Ok(LuaValue::Boolean(true))
}

/// Whether two outlines trace the same shape, allowing for a different
/// starting vertex.
fn same_polygon(a: &[[f64; 2]], b: &[[f64; 2]]) -> bool {
  if a.len() != b.len() {
    return false;
  }
  let n = a.len();
  (0..n).any(|shift| {
    (0..n).all(|i| {
      let p = a[i];
      let q = b[(i + shift) % n];
      (p[0] - q[0]).abs() < 1e-9 && (p[1] - q[1]).abs() < 1e-9
    })
  })
}

fn point_in_region(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let Some(point) = a.vec2("point") else {
    return a.err("point must be a 2D point");
  };
  let region = read_region(a, "region")?;
  let eps = a.num_or("eps", EPS);
  let _ = lua;
  let mut count = 0;
  for path in &region {
    match point_in_polygon(point, path, eps) {
      0 => return Ok(LuaValue::Number(0.0)),
      1 => count += 1,
      _ => {}
    }
  }
  Ok(LuaValue::Number(if count % 2 == 1 { 1.0 } else { -1.0 }))
}

fn hull_region(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let region = read_region(a, "region")?;
  let points: Vec<[f64; 2]> = region.into_iter().flatten().collect();
  let hull = convex_hull2(&points);
  Val::list(hull.iter().map(|p| Val::vec(*p))).to_lua(lua)
}

/// The convex hull of a point set, counter-clockwise, by Andrew's monotone
/// chain.
pub fn convex_hull2(points: &[[f64; 2]]) -> Vec<[f64; 2]> {
  if points.len() < 3 {
    return points.to_vec();
  }
  let mut pts = points.to_vec();
  pts.sort_by(|a, b| a[0].total_cmp(&b[0]).then(a[1].total_cmp(&b[1])));
  pts.dedup_by(|a, b| (a[0] - b[0]).abs() < EPS && (a[1] - b[1]).abs() < EPS);
  if pts.len() < 3 {
    return pts;
  }
  let build = |iter: &mut dyn Iterator<Item = [f64; 2]>| -> Vec<[f64; 2]> {
    let mut chain: Vec<[f64; 2]> = Vec::new();
    for p in iter {
      while chain.len() >= 2 {
        let n = chain.len();
        if cross2(sub2(chain[n - 1], chain[n - 2]), sub2(p, chain[n - 2]))
          <= 0.0
        {
          chain.pop();
        } else {
          break;
        }
      }
      chain.push(p);
    }
    chain.pop();
    chain
  };
  let mut lower = build(&mut pts.iter().copied());
  let upper = build(&mut pts.iter().rev().copied());
  lower.extend(upper);
  lower
}

fn exclusive_or(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let Some(v) = a.val("regions") else {
    return a.err("regions is required");
  };
  // Either a list of regions, or two regions given one after the other.
  let mut regions: Vec<Vec<Vec<[f64; 2]>>> = Vec::new();
  if let Some(items) = v.as_list() {
    for item in items {
      match region_of(item) {
        Some(r) => regions.push(r),
        None => return a.err("every entry must be a region"),
      }
    }
  }
  for name in ["b", "c"] {
    if let Some(v) = a.val(name)
      && let Some(r) = region_of(&v)
    {
      regions.push(r);
    }
  }
  if regions.is_empty() {
    return write_region(lua, &[]);
  }
  let out = regions
    .into_iter()
    .reduce(|acc, r| combine_outlines(&acc, &r, AreaOp::ExclusiveOr))
    .unwrap_or_default();
  write_region(lua, &out)
}

/// Cut both regions wherever they cross, so every crossing becomes a vertex.
fn split_region_at_region_crossings(
  lua: &Lua,
  a: &Args,
) -> LuaResult<LuaValue> {
  let r1 = read_region(a, "region1")?;
  let r2 = read_region(a, "region2")?;
  let closed1 = a.bool_or("closed1", true);
  let closed2 = a.bool_or("closed2", true);
  let eps = a.num_or("eps", EPS);

  let split = |region: &[Vec<[f64; 2]>],
               other: &[Vec<[f64; 2]>],
               closed: bool,
               other_closed: bool| {
    Val::list(region.iter().map(|path| {
      Val::list(
        split_path(path, other, closed, other_closed, eps)
          .iter()
          .map(|piece| Val::list(piece.iter().map(|p| Val::vec(*p)))),
      )
    }))
  };
  Val::list([
    split(&r1, &r2, closed1, closed2),
    split(&r2, &r1, closed2, closed1),
  ])
  .to_lua(lua)
}

/// One path cut into pieces wherever another region's outlines cross it.
fn split_path(
  path: &[[f64; 2]],
  other: &[Vec<[f64; 2]>],
  closed: bool,
  other_closed: bool,
  eps: f64,
) -> Vec<Vec<[f64; 2]>> {
  let n = path.len();
  let segs = if closed { n } else { n - 1 };
  let mut pieces: Vec<Vec<[f64; 2]>> = Vec::new();
  let mut current: Vec<[f64; 2]> = vec![path[0]];
  for i in 0..segs {
    let a1 = path[i];
    let a2 = path[(i + 1) % n];
    // Every crossing along this segment, in the order they are met.
    let mut hits: Vec<f64> = Vec::new();
    for poly in other {
      let m = poly.len();
      let other_segs = if other_closed { m } else { m - 1 };
      for j in 0..other_segs {
        if let Some((t, u)) =
          segment_intersection(a1, a2, poly[j], poly[(j + 1) % m])
          && t > eps
          && t < 1.0 - eps
          && u > -eps
          && u < 1.0 + eps
        {
          hits.push(t);
        }
      }
    }
    hits.sort_by(f64::total_cmp);
    hits.dedup_by(|x, y| (*x - *y).abs() < eps);
    for t in hits {
      let p = [a1[0] + (a2[0] - a1[0]) * t, a1[1] + (a2[1] - a1[1]) * t];
      current.push(p);
      pieces.push(std::mem::replace(&mut current, vec![p]));
    }
    current.push(a2);
  }
  if current.len() > 1 {
    pieces.push(current);
  }
  if pieces.is_empty() {
    pieces.push(path.to_vec());
  }
  pieces
}

/// Draw a region's outlines so they can be looked at.
///
/// The vertices become small dots and the edges thin bars, which is how a
/// region with a wrongly wound hole or a stray outline gives itself away.
fn debug_region(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let region = read_region(a, "region")?;
  let size = a.num_or("size", 1.0);
  let want_vertices = a.bool_or("vertices", true);
  let want_edges = a.bool_or("edges", true);

  let mut parts: Vec<crate::scad_export::ScadNode> = Vec::new();
  for path in &region {
    for (i, p) in path.iter().enumerate() {
      if want_vertices {
        parts.push(crate::scad_export::ScadNode::Translate {
          x: p[0] as f32,
          y: p[1] as f32,
          z: 0.0,
          child: Box::new(crate::scad_export::ScadNode::Sphere {
            r: (size / 2.0) as f32,
            segments: 12,
          }),
        });
      }
      if want_edges {
        let q = path[(i + 1) % path.len()];
        parts.push(bar(*p, q, size / 3.0));
      }
    }
  }
  let node = crate::scad_export::ScadNode::Union(parts);
  let scad = crate::bosl::bosl_node_with_children(
    "std.scad",
    "debug_region",
    a.scad_args().to_string(),
    vec![],
    Some(node),
  );
  Ok(LuaValue::UserData(lua.create_userdata(
    crate::geometry::CsgGeometry {
      name: None,
      mesh: None,
      color: None,
      scad: Some(scad),
    },
  )?))
}

/// A thin bar from one point to another, for drawing an edge.
fn bar(p: [f64; 2], q: [f64; 2], width: f64) -> crate::scad_export::ScadNode {
  use crate::scad_export::ScadNode;
  let d = sub2(q, p);
  let len = (d[0] * d[0] + d[1] * d[1]).sqrt();
  let ang = d[1].atan2(d[0]).to_degrees();
  ScadNode::Translate {
    x: p[0] as f32,
    y: p[1] as f32,
    z: 0.0,
    child: Box::new(ScadNode::Rotate {
      x: 0.0,
      y: 0.0,
      z: ang as f32,
      child: Box::new(ScadNode::Translate {
        x: (len / 2.0) as f32,
        y: 0.0,
        z: 0.0,
        child: Box::new(ScadNode::Cube {
          w: len as f32,
          d: width as f32,
          h: width as f32,
          center: true,
        }),
      }),
    }),
  }
}

pub fn register(lua: &Lua, bosl: &mlua::Table) -> LuaResult<()> {
  register_all(
    lua,
    bosl,
    &[
      ("is_region", &["x"], is_region as PureFn),
      ("is_valid_region", &["region", "eps"], is_valid_region),
      ("is_region_simple", &["region", "eps"], is_region_simple),
      ("make_region", &["polys", "nonzero", "eps"], make_region),
      ("region", &["r", "anchor", "spin", "cp", "atype"], region_fn),
      ("region_area", &["region"], region_area),
      ("region_parts", &["region"], region_parts),
      (
        "are_regions_equal",
        &["region1", "region2", "either_winding"],
        are_regions_equal,
      ),
      (
        "point_in_region",
        &["point", "region", "eps"],
        point_in_region,
      ),
      ("hull_region", &["region"], hull_region),
      ("exclusive_or", &["regions", "b", "c", "eps"], exclusive_or),
      (
        "split_region_at_region_crossings",
        &["region1", "region2", "closed1", "closed2", "eps"],
        split_region_at_region_crossings,
      ),
      (
        "debug_region",
        &["region", "vertices", "edges", "convexity", "size"],
        debug_region,
      ),
    ],
  )
}

#[cfg(test)]
mod tests {
  use super::*;

  fn square(s: f64) -> Vec<[f64; 2]> {
    vec![[0.0, 0.0], [s, 0.0], [s, s], [0.0, s]]
  }

  fn moved(path: &[[f64; 2]], dx: f64) -> Vec<[f64; 2]> {
    path.iter().map(|p| [p[0] + dx, p[1]]).collect()
  }

  #[test]
  fn a_point_inside_two_nested_outlines_is_in_the_hole() {
    let outer = square(10.0);
    let inner: Vec<[f64; 2]> = square(4.0)
      .iter()
      .map(|p| [p[0] + 3.0, p[1] + 3.0])
      .collect();
    let region = [outer, inner];
    // Inside the outer only: in the region.
    let mut count = 0;
    for path in &region {
      if point_in_polygon([1.0, 1.0], path, EPS) > 0 {
        count += 1;
      }
    }
    assert_eq!(count % 2, 1);
    // Inside both: in the hole.
    count = 0;
    for path in &region {
      if point_in_polygon([5.0, 5.0], path, EPS) > 0 {
        count += 1;
      }
    }
    assert_eq!(count % 2, 0);
  }

  #[test]
  fn a_region_of_two_separate_squares_has_both_their_areas() {
    let region = vec![square(10.0), moved(&square(8.0), 20.0)];
    let parts = split_into_parts(&region);
    let area: f64 = parts.iter().flatten().map(|p| -signed_area(p)).sum();
    assert!((area - 164.0).abs() < 1e-9, "{area}");
  }

  #[test]
  fn a_hole_is_taken_off_the_area_it_sits_in() {
    let inner: Vec<[f64; 2]> = square(4.0)
      .iter()
      .map(|p| [p[0] + 3.0, p[1] + 3.0])
      .collect();
    let region = vec![square(10.0), inner];
    let parts = split_into_parts(&region);
    assert_eq!(parts.len(), 1, "one part, with a hole in it");
    let area: f64 = parts.iter().flatten().map(|p| -signed_area(p)).sum();
    assert!((area - (100.0 - 16.0)).abs() < 1e-9, "{area}");
  }

  #[test]
  fn a_figure_eight_is_not_a_simple_path() {
    let bowtie = [[0.0, 0.0], [10.0, 10.0], [10.0, 0.0], [0.0, 10.0]];
    assert!(!is_path_simple(&bowtie, true, EPS));
    assert!(is_path_simple(&square(10.0), true, EPS));
  }

  #[test]
  fn touching_outlines_make_a_region_invalid() {
    // Two squares sharing an edge cross each other's outline.
    let a = square(10.0);
    let b = moved(&square(10.0), 10.0);
    assert!(paths_intersect(&a, &b, EPS));
    // Well clear of each other, they do not.
    let c = moved(&square(10.0), 30.0);
    assert!(!paths_intersect(&a, &c, EPS));
  }

  #[test]
  fn two_overlapping_squares_make_a_region_of_what_only_one_covers() {
    // BOSL2 builds a region by exclusive-or, so the overlap drops out and
    // what is left is the two end strips. Checked against BOSL2, which
    // reports the same 100 for this pair.
    let a = square(10.0);
    let b = moved(&square(10.0), 5.0);
    let out = combine_outlines(&[a], &[b], AreaOp::ExclusiveOr);
    assert_eq!(out.len(), 2, "{out:?}");
    let area: f64 = out.iter().map(|p| signed_area(p).abs()).sum();
    assert!((area - 100.0).abs() < 1e-6, "{area}");
  }

  #[test]
  fn overlapping_squares_union_into_one_outline() {
    let a = square(10.0);
    let b = moved(&square(10.0), 5.0);
    let out = combine_outlines(&[a], &[b], AreaOp::Union);
    assert_eq!(out.len(), 1, "{out:?}");
    let area: f64 = out.iter().map(|p| signed_area(p).abs()).sum();
    assert!((area - 150.0).abs() < 1e-6, "{area}");
  }

  #[test]
  fn exclusive_or_leaves_out_what_the_two_share() {
    let a = square(10.0);
    let b = moved(&square(10.0), 5.0);
    let out = combine_outlines(&[a], &[b], AreaOp::ExclusiveOr);
    let area: f64 = out.iter().map(|p| signed_area(p).abs()).sum();
    // 150 covered by either, less the 50 they share, counted once each way.
    assert!((area - 100.0).abs() < 1e-6, "{area}");
  }

  #[test]
  fn the_hull_of_a_ring_is_its_outer_square() {
    let inner: Vec<[f64; 2]> = square(4.0)
      .iter()
      .map(|p| [p[0] + 3.0, p[1] + 3.0])
      .collect();
    let hull = convex_hull2(&[square(10.0), inner].concat());
    assert_eq!(hull.len(), 4, "{hull:?}");
    let area = signed_area(&hull).abs();
    assert!((area - 100.0).abs() < 1e-9, "{area}");
  }
}
