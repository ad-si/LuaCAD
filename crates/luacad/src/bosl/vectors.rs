//! BOSL2's `vectors.scad`: vector arithmetic, angles and point searches.
//!
//! Anything that returns an index into a list returns an OpenSCAD-style
//! index, counting from zero, so it can be handed straight to
//! [`select`](crate::bosl::lists) and the rest of the list functions. Reading
//! the element out in Lua needs the usual `list[i + 1]`.

use mlua::{Lua, Result as LuaResult, Value as LuaValue};

use crate::bosl::value::{Args, PureFn, Val, num_list, register_all};

const EPS: f64 = 1e-9;

fn norm_of(v: &[f64]) -> f64 {
  v.iter().map(|x| x * x).sum::<f64>().sqrt()
}

fn dist(a: &[f64], b: &[f64]) -> f64 {
  a.iter()
    .zip(b.iter())
    .map(|(x, y)| (x - y) * (x - y))
    .sum::<f64>()
    .sqrt()
}

fn is_vector(_lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let ok = match a.val("v") {
    Some(v) => {
      let numeric = v.as_vec();
      match (numeric, a.num("length")) {
        (Some(v), Some(n)) => v.len() == n as usize && !v.is_empty(),
        (Some(v), None) => !v.is_empty(),
        (None, _) => false,
      }
    }
    None => false,
  };
  Ok(LuaValue::Boolean(ok))
}

fn add_scalar(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let v = a.need_vec("v")?;
  let s = a.need_num("s")?;
  num_list(lua, &v.iter().map(|x| x + s).collect::<Vec<_>>())
}

/// The component-wise binary operations, which all share a shape.
fn componentwise(
  lua: &Lua,
  a: &Args,
  op: fn(f64, f64) -> f64,
) -> LuaResult<LuaValue> {
  let v1 = a.need_vec("v1")?;
  let v2 = a.need_vec("v2")?;
  if v1.len() != v2.len() {
    return a.err("both vectors must be the same length");
  }
  num_list(
    lua,
    &v1
      .iter()
      .zip(v2.iter())
      .map(|(x, y)| op(*x, *y))
      .collect::<Vec<_>>(),
  )
}

fn v_mul(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  componentwise(lua, a, |x, y| x * y)
}

fn v_div(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  componentwise(lua, a, |x, y| x / y)
}

/// The component-wise unary operations.
fn elementwise(lua: &Lua, a: &Args, op: fn(f64) -> f64) -> LuaResult<LuaValue> {
  a.need_val("v")?.map_num(&op).to_lua(lua)
}

fn v_abs(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  elementwise(lua, a, f64::abs)
}

fn v_ceil(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  elementwise(lua, a, f64::ceil)
}

fn v_floor(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  elementwise(lua, a, f64::floor)
}

fn v_round(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  elementwise(lua, a, |x| if x < 0.0 { -(-x).round() } else { x.round() })
}

/// Interpolate a table of `[x, value]` pairs at `x`.
fn v_lookup(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let x = a.need_num("x")?;
  let Some(rows) = a.need_val("v")?.as_list().map(|s| s.to_vec()) else {
    return a.err("v must be a list of [x, value] pairs");
  };
  if rows.is_empty() {
    return a.err("v cannot be empty");
  }
  let key =
    |r: &Val| -> Option<f64> { r.as_list()?.first().and_then(|k| k.as_num()) };
  let value = |r: &Val| -> Option<Val> { r.as_list()?.get(1).cloned() };

  let mut sorted = rows.clone();
  sorted
    .sort_by(|p, q| key(p).unwrap_or(0.0).total_cmp(&key(q).unwrap_or(0.0)));
  let first = key(&sorted[0]).unwrap_or(0.0);
  let last = key(&sorted[sorted.len() - 1]).unwrap_or(0.0);
  if x <= first {
    return match value(&sorted[0]) {
      Some(v) => v.to_lua(lua),
      None => a.err("each entry of v must be [x, value]"),
    };
  }
  if x >= last {
    return match value(&sorted[sorted.len() - 1]) {
      Some(v) => v.to_lua(lua),
      None => a.err("each entry of v must be [x, value]"),
    };
  }
  for pair in sorted.windows(2) {
    let (lo, hi) = (key(&pair[0]), key(&pair[1]));
    let (Some(lo), Some(hi)) = (lo, hi) else {
      return a.err("each entry of v must be [x, value]");
    };
    if x >= lo && x <= hi {
      let (Some(vlo), Some(vhi)) = (value(&pair[0]), value(&pair[1])) else {
        return a.err("each entry of v must be [x, value]");
      };
      if (hi - lo).abs() < EPS {
        return vlo.to_lua(lua);
      }
      let u = (x - lo) / (hi - lo);
      return match vlo.scale(1.0 - u).add(&vhi.scale(u)) {
        Some(v) => v.to_lua(lua),
        None => a.err("the values in v are not all the same shape"),
      };
    }
  }
  a.err("could not interpolate at the given x")
}

fn unit(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let v = a.need_vec("v")?;
  let n = norm_of(&v);
  if n < EPS {
    // BOSL2 lets the caller nominate what a zero vector should become
    // instead of failing, which is what keeps the geometry code branchless.
    return match a.val("error") {
      Some(fallback) => fallback.to_lua(lua),
      None => a.err("cannot normalize a zero-length vector"),
    };
  }
  num_list(lua, &v.iter().map(|x| x / n).collect::<Vec<_>>())
}

fn v_theta(_lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let v = a.need_vec("v")?;
  if v.len() < 2 {
    return a.err("v must have at least two components");
  }
  Ok(LuaValue::Number(v[1].atan2(v[0]).to_degrees()))
}

fn vector_angle(_lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  // Either two vectors, three points naming an angle at the middle one, or a
  // list of either.
  let vals: Vec<Val> =
    ["v1", "v2", "v3"].iter().filter_map(|n| a.val(n)).collect();
  let vectors: Vec<Vec<f64>> = match vals.len() {
    1 => match vals[0].as_matrix() {
      Some(m) => m,
      None => return a.err("give two vectors, or a list of two or three"),
    },
    _ => {
      let mut out = Vec::new();
      for v in &vals {
        match v.as_vec() {
          Some(v) => out.push(v),
          None => return a.err("give two vectors, or three points"),
        }
      }
      out
    }
  };

  let (u, w) = match vectors.len() {
    2 => (vectors[0].clone(), vectors[1].clone()),
    // Three points measure the angle at the middle one.
    3 => (
      vectors[0]
        .iter()
        .zip(vectors[1].iter())
        .map(|(x, y)| x - y)
        .collect(),
      vectors[2]
        .iter()
        .zip(vectors[1].iter())
        .map(|(x, y)| x - y)
        .collect(),
    ),
    _ => return a.err("give two vectors, or three points"),
  };
  let (nu, nw) = (norm_of(&u), norm_of(&w));
  if nu < EPS || nw < EPS {
    return a.err("cannot take the angle of a zero-length vector");
  }
  let dot: f64 = u.iter().zip(w.iter()).map(|(x, y)| x * y).sum();
  let c = (dot / (nu * nw)).clamp(-1.0, 1.0);
  Ok(LuaValue::Number(c.acos().to_degrees()))
}

fn vector_axis(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let vals: Vec<Vec<f64>> = ["v1", "v2", "v3"]
    .iter()
    .filter_map(|n| a.val(n))
    .filter_map(|v| v.as_vec())
    .collect();
  let (u, w) = match vals.len() {
    2 => (
      crate::bosl::value::v3(&vals[0]),
      crate::bosl::value::v3(&vals[1]),
    ),
    3 => {
      let (p0, p1, p2) = (
        crate::bosl::value::v3(&vals[0]),
        crate::bosl::value::v3(&vals[1]),
        crate::bosl::value::v3(&vals[2]),
      );
      (
        [p0[0] - p1[0], p0[1] - p1[1], p0[2] - p1[2]],
        [p2[0] - p1[0], p2[1] - p1[1], p2[2] - p1[2]],
      )
    }
    _ => return a.err("give two vectors, or three points"),
  };
  let axis = crate::bosl::vecmath::vector_axis(u, w);
  num_list(lua, &axis)
}

fn vector_bisect(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let v1 = a.need_vec("v1")?;
  let v2 = a.need_vec("v2")?;
  if v1.len() != v2.len() {
    return a.err("both vectors must be the same length");
  }
  let (n1, n2) = (norm_of(&v1), norm_of(&v2));
  if n1 < EPS || n2 < EPS {
    return a.err("cannot bisect a zero-length vector");
  }
  // Normalising first and adding is the bisector; it only fails when the two
  // point exactly opposite ways, where no unique bisector exists.
  let sum: Vec<f64> = v1
    .iter()
    .zip(v2.iter())
    .map(|(x, y)| x / n1 + y / n2)
    .collect();
  let n = norm_of(&sum);
  if n < EPS {
    return Ok(LuaValue::Nil);
  }
  num_list(lua, &sum.iter().map(|x| x / n).collect::<Vec<_>>())
}

fn vector_perp(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let v = a.need_vec("v")?;
  let w = a.need_vec("w")?;
  if v.len() != w.len() {
    return a.err("both vectors must be the same length");
  }
  let vv: f64 = v.iter().map(|x| x * x).sum();
  if vv < EPS {
    return a.err("cannot take a component perpendicular to a zero vector");
  }
  let wv: f64 = w.iter().zip(v.iter()).map(|(x, y)| x * y).sum();
  num_list(
    lua,
    &w.iter()
      .zip(v.iter())
      .map(|(x, y)| x - y * wv / vv)
      .collect::<Vec<_>>(),
  )
}

/// The index of the point nearest to or furthest from `pt`.
fn extreme_point(_lua: &Lua, a: &Args, furthest: bool) -> LuaResult<LuaValue> {
  let pt = a.need_vec("pt")?;
  let points = a.need_matrix("points")?;
  if points.is_empty() {
    return a.err("points cannot be empty");
  }
  let mut best = 0usize;
  let mut best_d = dist(&points[0], &pt);
  for (i, p) in points.iter().enumerate().skip(1) {
    let d = dist(p, &pt);
    if (furthest && d > best_d) || (!furthest && d < best_d) {
      best = i;
      best_d = d;
    }
  }
  Ok(LuaValue::Number(best as f64))
}

fn closest_point(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  extreme_point(lua, a, false)
}

fn furthest_point(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  extreme_point(lua, a, true)
}

/// The points within `r` of each query point.
fn vector_search(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let r = a.need_num("r")?;
  if r < 0.0 {
    return a.err("the search radius cannot be negative");
  }
  let target = a.need_matrix("target")?;
  let query = a.need_val("query")?;

  let search_one = |q: &[f64]| -> Val {
    Val::vec(
      target
        .iter()
        .enumerate()
        .filter(|(_, p)| dist(p, q) <= r)
        .map(|(i, _)| i as f64),
    )
  };

  // A single point gives one list of indices; a list of points gives one
  // such list each.
  match query.as_vec() {
    Some(q) if !q.is_empty() => search_one(&q).to_lua(lua),
    _ => {
      let Some(points) = query.as_matrix() else {
        return a.err("query must be a point or a list of points");
      };
      Val::list(points.iter().map(|q| search_one(q))).to_lua(lua)
    }
  }
}

/// A search structure for repeated queries.
///
/// BOSL2 builds a balanced tree here. This returns the points unchanged,
/// which [`vector_search`] and [`vector_nearest`] accept just the same; they
/// scan the list rather than descending a tree, so the answers match and only
/// the speed on very large point sets differs.
fn vector_search_tree(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  a.need_val("points")?.to_lua(lua)
}

fn vector_nearest(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let query = a.need_vec("query")?;
  let k = a.need_num("k")? as usize;
  let target = a.need_matrix("target")?;
  if k == 0 {
    return a.err("k must be at least 1");
  }
  if k > target.len() {
    return a.err("more results were asked for than there are points");
  }
  let mut order: Vec<usize> = (0..target.len()).collect();
  order.sort_by(|i, j| {
    dist(&target[*i], &query).total_cmp(&dist(&target[*j], &query))
  });
  num_list(
    lua,
    &order[..k].iter().map(|i| *i as f64).collect::<Vec<_>>(),
  )
}

fn pointlist_bounds(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let pts = a.need_matrix("pts")?;
  if pts.is_empty() {
    return a.err("pts cannot be empty");
  }
  let dim = pts.iter().map(|p| p.len()).max().unwrap_or(0);
  let mut lo = vec![f64::INFINITY; dim];
  let mut hi = vec![f64::NEG_INFINITY; dim];
  for p in &pts {
    for i in 0..dim {
      let v = p.get(i).copied().unwrap_or(0.0);
      lo[i] = lo[i].min(v);
      hi[i] = hi[i].max(v);
    }
  }
  Val::list([Val::vec(lo), Val::vec(hi)]).to_lua(lua)
}

fn fit_to_box(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let pts = a.need_matrix("pts")?;
  let lo_box = a.need_vec("lo")?;
  let hi_box = a.need_vec("hi")?;
  if pts.is_empty() {
    return a.err("pts cannot be empty");
  }
  let dim = lo_box.len().min(hi_box.len());
  let mut lo = vec![f64::INFINITY; dim];
  let mut hi = vec![f64::NEG_INFINITY; dim];
  for p in &pts {
    for i in 0..dim {
      let v = p.get(i).copied().unwrap_or(0.0);
      lo[i] = lo[i].min(v);
      hi[i] = hi[i].max(v);
    }
  }
  // One scale for every axis, so the points keep their proportions.
  let mut scale = f64::INFINITY;
  for i in 0..dim {
    let span = hi[i] - lo[i];
    let target = hi_box[i] - lo_box[i];
    if span > EPS {
      scale = scale.min(target / span);
    }
  }
  if !scale.is_finite() {
    scale = 1.0;
  }
  Val::list(pts.iter().map(|p| {
    Val::vec((0..dim).map(|i| {
      let v = p.get(i).copied().unwrap_or(0.0);
      let centred = v - (lo[i] + hi[i]) / 2.0;
      centred * scale + (lo_box[i] + hi_box[i]) / 2.0
    }))
  }))
  .to_lua(lua)
}

pub fn register(lua: &Lua, bosl: &mlua::Table) -> LuaResult<()> {
  register_all(
    lua,
    bosl,
    &[
      ("is_vector", &["v", "length"], is_vector as PureFn),
      ("add_scalar", &["v", "s"], add_scalar),
      ("v_mul", &["v1", "v2"], v_mul),
      ("v_div", &["v1", "v2"], v_div),
      ("v_abs", &["v"], v_abs),
      ("v_ceil", &["v"], v_ceil),
      ("v_floor", &["v"], v_floor),
      ("v_round", &["v"], v_round),
      ("v_lookup", &["x", "v"], v_lookup),
      ("unit", &["v", "error"], unit),
      ("v_theta", &["v"], v_theta),
      ("vector_angle", &["v1", "v2", "v3"], vector_angle),
      ("vector_axis", &["v1", "v2", "v3"], vector_axis),
      ("vector_bisect", &["v1", "v2"], vector_bisect),
      ("vector_perp", &["v", "w"], vector_perp),
      ("closest_point", &["pt", "points"], closest_point),
      ("furthest_point", &["pt", "points"], furthest_point),
      ("vector_search", &["query", "r", "target"], vector_search),
      (
        "vector_search_tree",
        &["points", "leafsize"],
        vector_search_tree,
      ),
      ("vector_nearest", &["query", "k", "target"], vector_nearest),
      ("pointlist_bounds", &["pts"], pointlist_bounds),
      ("fit_to_box", &["pts", "lo", "hi"], fit_to_box),
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
  fn unit_scales_a_vector_to_length_one() {
    let v: Vec<f64> = eval("return bosl.unit({3, 4})");
    assert_eq!(v, vec![0.6, 0.8]);
  }

  #[test]
  fn normalizing_a_zero_vector_fails_unless_a_fallback_is_given() {
    let lua = Lua::new();
    register_bosl(&lua).unwrap();
    assert!(
      lua
        .load("return bosl.unit({0, 0})")
        .eval::<mlua::Value>()
        .is_err()
    );
    let v: Vec<f64> = eval("return bosl.unit({0, 0}, {1, 0})");
    assert_eq!(v, vec![1.0, 0.0]);
  }

  #[test]
  fn component_wise_products_and_quotients() {
    let v: Vec<f64> = eval("return bosl.v_mul({2, 3}, {4, 5})");
    assert_eq!(v, vec![8.0, 15.0]);
    let v: Vec<f64> = eval("return bosl.v_div({8, 15}, {4, 5})");
    assert_eq!(v, vec![2.0, 3.0]);
  }

  #[test]
  fn rounding_functions_apply_to_every_component() {
    let v: Vec<f64> = eval("return bosl.v_floor({1.7, -1.2})");
    assert_eq!(v, vec![1.0, -2.0]);
    let v: Vec<f64> = eval("return bosl.v_ceil({1.2, -1.7})");
    assert_eq!(v, vec![2.0, -1.0]);
    let v: Vec<f64> = eval("return bosl.v_abs({-3, 4})");
    assert_eq!(v, vec![3.0, 4.0]);
  }

  #[test]
  fn angles_come_back_in_degrees() {
    let t: f64 = eval("return bosl.v_theta({0, 1})");
    assert_eq!(t, 90.0);
    let ang: f64 = eval("return bosl.vector_angle({1,0,0}, {0,1,0})");
    assert_eq!(ang, 90.0);
    // Three points measure the angle at the middle one.
    let ang: f64 = eval("return bosl.vector_angle({1,0}, {0,0}, {0,1})");
    assert_eq!(ang, 90.0);
  }

  #[test]
  fn the_bisector_lies_halfway_between_two_directions() {
    let v: Vec<f64> = eval("return bosl.vector_bisect({1,0,0}, {0,1,0})");
    let s = 1.0 / 2f64.sqrt();
    assert!((v[0] - s).abs() < 1e-9 && (v[1] - s).abs() < 1e-9, "{v:?}");
  }

  #[test]
  fn vector_perp_removes_the_parallel_component() {
    let v: Vec<f64> = eval("return bosl.vector_perp({1,0}, {3,4})");
    assert_eq!(v, vec![0.0, 4.0]);
  }

  #[test]
  fn point_searches_return_openscad_style_indices() {
    let i: f64 = eval("return bosl.closest_point({0,0}, {{5,5},{1,1},{9,9}})");
    assert_eq!(i, 1.0);
    let i: f64 = eval("return bosl.furthest_point({0,0}, {{5,5},{1,1},{9,9}})");
    assert_eq!(i, 2.0);
    // The index feeds straight into the list functions.
    let p: Vec<f64> = eval(
      "local pts = {{5,5},{1,1},{9,9}}
       return bosl.select(pts, bosl.closest_point({0,0}, pts))",
    );
    assert_eq!(p, vec![1.0, 1.0]);
  }

  #[test]
  fn radius_search_finds_every_point_inside_it() {
    let idx: Vec<f64> =
      eval("return bosl.vector_search({0,0}, 2, {{0,1},{5,5},{1,1},{9,9}})");
    assert_eq!(idx, vec![0.0, 2.0]);
  }

  #[test]
  fn nearest_search_orders_by_distance() {
    let idx: Vec<f64> =
      eval("return bosl.vector_nearest({0,0}, 2, {{5,5},{1,1},{0,2}})");
    assert_eq!(idx, vec![1.0, 2.0]);
  }

  #[test]
  fn a_search_tree_can_stand_in_for_the_point_list() {
    let idx: Vec<f64> = eval(
      "local t = bosl.vector_search_tree({{0,1},{5,5},{1,1}})
       return bosl.vector_search({0,0}, 2, t)",
    );
    assert_eq!(idx, vec![0.0, 2.0]);
  }

  #[test]
  fn bounds_come_back_as_a_low_and_a_high_corner() {
    let b: Vec<Vec<f64>> =
      eval("return bosl.pointlist_bounds({{1,5},{-2,3},{4,0}})");
    assert_eq!(b, vec![vec![-2.0, 0.0], vec![4.0, 5.0]]);
  }

  #[test]
  fn lookup_interpolates_between_table_entries() {
    let v: f64 = eval("return bosl.v_lookup(5, {{0,0},{10,100}})");
    assert_eq!(v, 50.0);
    // It carries vector values through as readily as numbers.
    let v: Vec<f64> = eval("return bosl.v_lookup(5, {{0,{0,0}},{10,{10,20}}})");
    assert_eq!(v, vec![5.0, 10.0]);
  }

  #[test]
  fn fitting_to_a_box_keeps_the_proportions() {
    let pts: Vec<Vec<f64>> =
      eval("return bosl.fit_to_box({{0,0},{2,1}}, {0,0}, {10,10})");
    // The wider axis is what limits the scale, so x spans the full box.
    assert!((pts[0][0] - 0.0).abs() < 1e-9, "{pts:?}");
    assert!((pts[1][0] - 10.0).abs() < 1e-9, "{pts:?}");
    assert!((pts[1][1] - pts[0][1] - 5.0).abs() < 1e-9, "{pts:?}");
  }

  #[test]
  fn is_vector_checks_shape_and_optionally_length() {
    assert!(eval::<bool>("return bosl.is_vector({1,2,3})"));
    assert!(eval::<bool>("return bosl.is_vector({1,2,3}, 3)"));
    assert!(!eval::<bool>("return bosl.is_vector({1,2,3}, 2)"));
    assert!(!eval::<bool>("return bosl.is_vector({{1,2}})"));
    assert!(!eval::<bool>("return bosl.is_vector(5)"));
  }
}
