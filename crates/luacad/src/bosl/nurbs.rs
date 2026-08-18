//! BOSL2's `nurbs.scad`: NURBS curves and surface patches.
//!
//! A NURBS is a Bézier with two extra freedoms. A *knot vector* says where
//! along the parameter each control point takes over, so one curve can be
//! made of many spans without splitting it up; and a *weight* on each control
//! point pulls the curve towards it, which is what lets a NURBS trace a
//! circle exactly where a Bézier can only approximate one.
//!
//! Both are evaluated by the Cox–de Boor recursion: a point is a weighted
//! blend of the control points, and the blending weights are built up degree
//! by degree from the knots.

use mlua::{Lua, Result as LuaResult, Value as LuaValue};

use crate::bosl::value::{Args, PureFn, Val, register_all, v3};

/// How a curve's ends are treated.
#[derive(Clone, Copy, PartialEq, Eq)]
enum EndType {
  /// The curve starts and ends at its first and last control points.
  Clamped,
  /// The curve joins back on itself.
  Closed,
  /// The curve stops short of both ends, where the knots run out.
  Open,
}

impl EndType {
  fn parse(name: &str) -> Option<EndType> {
    match name {
      "clamped" => Some(EndType::Clamped),
      "closed" => Some(EndType::Closed),
      "open" => Some(EndType::Open),
      _ => None,
    }
  }
}

/// The knot vector a curve gets when none is given.
///
/// Clamped repeats the first and last knots `degree + 1` times, which is what
/// pins the curve to its end control points. Open and closed space them
/// evenly and let the ends fall where they may.
fn default_knots(n: usize, degree: usize, end: EndType) -> Vec<f64> {
  let count = n + degree + 1;
  match end {
    EndType::Clamped => (0..count)
      .map(|i| {
        let inner = (n - degree) as f64;
        ((i as f64 - degree as f64).clamp(0.0, inner)) / inner
      })
      .collect(),
    _ => (0..count)
      .map(|i| (i as f64 - degree as f64) / (n - degree) as f64)
      .collect(),
  }
}

/// The `i`th basis function of the given degree, at `u`.
///
/// Cox–de Boor: degree zero is one inside its own knot span and zero
/// elsewhere, and each higher degree blends the two below it.
fn basis(i: usize, degree: usize, u: f64, knots: &[f64]) -> f64 {
  if degree == 0 {
    // Spans are half-open, so that exactly one holds any given `u`. The
    // last one that is not empty closes at its right end too, or the very
    // end of the curve would fall outside every basis function at once.
    let last = last_span(knots);
    let in_span = if i == last {
      u >= knots[i] && u <= knots[i + 1]
    } else {
      u >= knots[i] && u < knots[i + 1]
    };
    return f64::from(in_span);
  }
  let mut total = 0.0;
  let d1 = knots[i + degree] - knots[i];
  if d1.abs() > 1e-12 {
    total += (u - knots[i]) / d1 * basis(i, degree - 1, u, knots);
  }
  let d2 = knots[i + degree + 1] - knots[i + 1];
  if d2.abs() > 1e-12 {
    total +=
      (knots[i + degree + 1] - u) / d2 * basis(i + 1, degree - 1, u, knots);
  }
  total
}

/// The last knot span with any width in it.
///
/// A clamped knot vector ends with repeats, so its final spans are empty;
/// the curve really stops at the last one that is not.
fn last_span(knots: &[f64]) -> usize {
  (0..knots.len() - 1)
    .rev()
    .find(|j| knots[j + 1] > knots[*j])
    .unwrap_or(0)
}

/// A curve, ready to evaluate.
struct Curve {
  control: Vec<[f64; 3]>,
  weights: Vec<f64>,
  knots: Vec<f64>,
  degree: usize,
  /// Where the curve actually runs, as knot values.
  span: (f64, f64),
}

impl Curve {
  fn new(
    control: Vec<[f64; 3]>,
    degree: usize,
    weights: Option<Vec<f64>>,
    knots: Option<Vec<f64>>,
    end: EndType,
  ) -> Result<Curve, String> {
    let mut control = control;
    if end == EndType::Closed {
      // A closed curve wraps its first `degree` control points round to the
      // end, so the join is as smooth as anywhere else along it.
      let head: Vec<[f64; 3]> = control[..degree.min(control.len())].to_vec();
      control.extend(head);
    }
    let n = control.len();
    if n < degree + 1 {
      return Err(format!(
        "a degree {degree} curve needs at least {} control points, not {n}",
        degree + 1
      ));
    }
    let weights = match weights {
      Some(w) if w.len() == n => w,
      Some(w) if end == EndType::Closed && w.len() + degree == n => {
        let mut w2 = w.clone();
        w2.extend(w[..degree].to_vec());
        w2
      }
      Some(_) => {
        return Err("there must be one weight per control point".into());
      }
      None => vec![1.0; n],
    };
    if weights.iter().any(|w| *w <= 0.0) {
      return Err("every weight must be positive".into());
    }
    let knots = match knots {
      Some(k) if k.len() == n + degree + 1 => k,
      Some(k) => {
        return Err(format!(
          "the knot vector must have {} entries, not {}",
          n + degree + 1,
          k.len()
        ));
      }
      None => default_knots(n, degree, end),
    };
    // Outside this range fewer than `degree + 1` basis functions overlap and
    // the curve is not defined.
    let span = (knots[degree], knots[n]);
    Ok(Curve {
      control,
      weights,
      knots,
      degree,
      span,
    })
  }

  /// The point at `u`, with `u` running from 0 to 1 over the whole curve.
  fn at(&self, u: f64) -> [f64; 3] {
    let t = self.span.0 + (self.span.1 - self.span.0) * u.clamp(0.0, 1.0);
    let mut num = [0.0; 3];
    let mut den = 0.0;
    for (i, control) in self.control.iter().enumerate() {
      let b = basis(i, self.degree, t, &self.knots) * self.weights[i];
      if b == 0.0 {
        continue;
      }
      for (k, out) in num.iter_mut().enumerate() {
        *out += control[k] * b;
      }
      den += b;
    }
    if den.abs() < 1e-12 {
      return *self.control.last().unwrap();
    }
    [num[0] / den, num[1] / den, num[2] / den]
  }
}

fn read_end_type(a: &Args, name: &str, which: usize) -> LuaResult<EndType> {
  let raw = match a.string(name) {
    Some(s) => s,
    None => match a.val(name) {
      // A patch takes one per direction.
      Some(Val::List(items)) => {
        let _ = items;
        match a.raw(name) {
          Some(LuaValue::Table(t)) => match t.get::<LuaValue>(which + 1) {
            Ok(LuaValue::String(s)) => {
              s.to_str().map(|s| s.to_string()).unwrap_or_default()
            }
            _ => "clamped".to_string(),
          },
          _ => "clamped".to_string(),
        }
      }
      _ => "clamped".to_string(),
    },
  };
  match EndType::parse(&raw) {
    Some(e) => Ok(e),
    None => a.err("type must be \"clamped\", \"closed\" or \"open\""),
  }
}

fn read_control(a: &Args, name: &str) -> LuaResult<(Vec<[f64; 3]>, usize)> {
  match a.val(name).and_then(|v| v.as_matrix()) {
    Some(m) if m.len() >= 2 => {
      let dim = m[0].len().clamp(2, 3);
      Ok((m.iter().map(|p| v3(p)).collect(), dim))
    }
    _ => a.err(format!("{name} must be a list of control points")),
  }
}

fn out_point(p: [f64; 3], dim: usize) -> Val {
  Val::vec(p[..dim].to_vec())
}

fn nurbs_curve(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let (control, dim) = read_control(a, "control")?;
  let degree = a.int("degree").unwrap_or(3).max(1) as usize;
  let end = read_end_type(a, "type", 0)?;
  let curve =
    Curve::new(control, degree, a.nums("weights"), a.nums("knots"), end)
      .or_else(|e| a.err(e))?;

  // Either a list of parameters, or a step count over the whole curve.
  if let Some(u) = a.val("u") {
    return match u {
      Val::Num(u) => out_point(curve.at(u), dim).to_lua(lua),
      other => match other.as_vec() {
        Some(us) => {
          Val::list(us.iter().map(|u| out_point(curve.at(*u), dim))).to_lua(lua)
        }
        None => a.err("u must be a number or a list of numbers"),
      },
    };
  }
  let steps = a.int("splinesteps").unwrap_or(16).max(1) as usize;
  // One sample per step along each span, plus the far end.
  let spans = curve.control.len() - degree;
  let total = steps * spans;
  Val::list(
    (0..=total).map(|i| out_point(curve.at(i as f64 / total as f64), dim)),
  )
  .to_lua(lua)
}

/// Read a patch: a rectangular grid of control points.
fn read_patch(a: &Args, name: &str) -> LuaResult<(Vec<Vec<[f64; 3]>>, usize)> {
  let Some(rows) = a.val(name).and_then(|v| v.as_list().map(|s| s.to_vec()))
  else {
    return a.err(format!("{name} must be a grid of control points"));
  };
  let mut out = Vec::with_capacity(rows.len());
  let mut dim = 3;
  for row in &rows {
    match row.as_matrix() {
      Some(m) if !m.is_empty() => {
        dim = m[0].len().clamp(2, 3);
        out.push(m.iter().map(|p| v3(p)).collect::<Vec<[f64; 3]>>());
      }
      _ => return a.err(format!("{name} must be a grid of control points")),
    }
  }
  if out.len() < 2 || out.iter().any(|r| r.len() != out[0].len()) {
    return a.err(format!("{name} must be a rectangular grid"));
  }
  Ok((out, dim))
}

fn is_nurbs_patch(_lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let ok = a
    .val("x")
    .and_then(|v| v.as_list().map(|s| s.to_vec()))
    .map(|rows| {
      rows.len() >= 2
        && rows.iter().all(|r| r.as_matrix().is_some())
        && rows[0].as_matrix().map(|m| m.len())
          == rows[rows.len() - 1].as_matrix().map(|m| m.len())
    })
    .unwrap_or(false);
  Ok(LuaValue::Boolean(ok))
}

/// The two degrees a patch is evaluated at, one per direction.
fn patch_degrees(a: &Args) -> (usize, usize) {
  match a.nums("degree") {
    Some(v) if v.len() >= 2 => (v[0] as usize, v[1] as usize),
    Some(v) if !v.is_empty() => (v[0] as usize, v[0] as usize),
    _ => (3, 3),
  }
}

/// The grid of points a patch is sampled at.
fn patch_grid(a: &Args) -> LuaResult<(Vec<Vec<[f64; 3]>>, usize)> {
  let (patch, dim) = read_patch(a, "patch")?;
  let (du, dv) = patch_degrees(a);
  let type_u = read_end_type(a, "type", 0)?;
  let type_v = read_end_type(a, "type", 1)?;
  let weights = a.val("weights").and_then(|v| v.as_matrix());

  // A surface is a curve of curves: each row is evaluated across, then the
  // results are evaluated down.
  let steps = a.int("splinesteps").unwrap_or(16).max(1) as usize;
  let rows: Vec<Curve> = patch
    .iter()
    .enumerate()
    .map(|(i, row)| {
      Curve::new(
        row.clone(),
        dv,
        weights.as_ref().and_then(|w| w.get(i).cloned()),
        None,
        type_v,
      )
    })
    .collect::<Result<Vec<_>, _>>()
    .or_else(|e| a.err(e))?;

  let across = steps * (patch[0].len() - dv.min(patch[0].len() - 1));
  let down = steps * (patch.len() - du.min(patch.len() - 1));
  let mut out: Vec<Vec<[f64; 3]>> = Vec::with_capacity(down + 1);
  for j in 0..=down {
    let v = j as f64 / down as f64;
    let column: Vec<[f64; 3]> = (0..=across)
      .map(|i| {
        let u = i as f64 / across as f64;
        // The point at (u, v) is where the row curves, sampled at u, are
        // themselves interpolated at v.
        let through: Vec<[f64; 3]> = rows.iter().map(|c| c.at(u)).collect();
        Curve::new(through, du, None, None, type_u)
          .map(|c| c.at(v))
          .unwrap_or([0.0; 3])
      })
      .collect();
    out.push(column);
  }
  Ok((out, dim))
}

fn nurbs_patch_points(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let (grid, dim) = patch_grid(a)?;
  Val::list(
    grid
      .iter()
      .map(|row| Val::list(row.iter().map(|p| out_point(*p, dim)))),
  )
  .to_lua(lua)
}

fn nurbs_vnf(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let (grid, _) = patch_grid(a)?;
  let type_u = read_end_type(a, "type", 0)?;
  let type_v = read_end_type(a, "type", 1)?;
  let vnf = crate::bosl::vnf::Vnf::vertex_array(
    &grid,
    crate::bosl::vnf::Caps::NONE,
    type_v == EndType::Closed,
    type_u == EndType::Closed,
  );
  crate::bosl::vnf_lua::write_vnf(lua, &vnf)
}

/// Draw a NURBS curve with its control polygon, so it can be looked at.
fn debug_nurbs(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let (control, _) = read_control(a, "control")?;
  let degree = a.int("degree").unwrap_or(3).max(1) as usize;
  let end = read_end_type(a, "type", 0)?;
  let width = a.num_or("width", 1.0);
  let curve = Curve::new(
    control.clone(),
    degree,
    a.nums("weights"),
    a.nums("knots"),
    end,
  )
  .or_else(|e| a.err(e))?;
  let steps = a.int("splinesteps").unwrap_or(16).max(1) as usize
    * (curve.control.len() - degree);
  let sampled: Vec<[f64; 3]> = (0..=steps)
    .map(|i| curve.at(i as f64 / steps as f64))
    .collect();

  let mut parts = bars(&sampled, width);
  parts.extend(bars(&control, width / 2.0));
  let scad = crate::bosl::bosl_node_with_children(
    "nurbs.scad",
    "debug_nurbs",
    a.scad_args().to_string(),
    vec![],
    Some(crate::scad_export::ScadNode::Union(parts)),
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

/// A run of thin bars along a polyline.
fn bars(path: &[[f64; 3]], width: f64) -> Vec<crate::scad_export::ScadNode> {
  use crate::scad_export::ScadNode;
  path
    .windows(2)
    .filter_map(|w| {
      let d = [w[1][0] - w[0][0], w[1][1] - w[0][1], w[1][2] - w[0][2]];
      let len = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
      if len < 1e-9 {
        return None;
      }
      Some(ScadNode::Translate {
        x: w[0][0] as f32,
        y: w[0][1] as f32,
        z: w[0][2] as f32,
        child: Box::new(ScadNode::Rotate {
          x: 0.0,
          y: (d[2] / len).clamp(-1.0, 1.0).acos().to_degrees() as f32,
          z: d[1].atan2(d[0]).to_degrees() as f32,
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

pub fn register(lua: &Lua, bosl: &mlua::Table) -> LuaResult<()> {
  register_all(
    lua,
    bosl,
    &[
      (
        "nurbs_curve",
        &[
          "control",
          "degree",
          "splinesteps",
          "u",
          "mult",
          "weights",
          "type",
          "knots",
        ],
        nurbs_curve as PureFn,
      ),
      ("is_nurbs_patch", &["x"], is_nurbs_patch),
      (
        "nurbs_patch_points",
        &[
          "patch",
          "degree",
          "splinesteps",
          "u",
          "v",
          "weights",
          "type",
          "mult",
          "knots",
        ],
        nurbs_patch_points,
      ),
      (
        "nurbs_vnf",
        &[
          "patch",
          "degree",
          "splinesteps",
          "weights",
          "type",
          "mult",
          "knots",
          "style",
        ],
        nurbs_vnf,
      ),
      (
        "debug_nurbs",
        &[
          "control",
          "degree",
          "splinesteps",
          "width",
          "size",
          "mult",
          "weights",
          "type",
          "knots",
          "show_weights",
          "show_knots",
          "show_index",
        ],
        debug_nurbs,
      ),
    ],
  )
}

#[cfg(test)]
mod tests {
  use super::*;

  fn line_control() -> Vec<[f64; 3]> {
    vec![
      [0.0, 0.0, 0.0],
      [10.0, 0.0, 0.0],
      [20.0, 0.0, 0.0],
      [30.0, 0.0, 0.0],
    ]
  }

  #[test]
  fn a_clamped_curve_starts_and_ends_on_its_control_points() {
    let c =
      Curve::new(line_control(), 3, None, None, EndType::Clamped).unwrap();
    let start = c.at(0.0);
    let end = c.at(1.0);
    assert!((start[0] - 0.0).abs() < 1e-9, "{start:?}");
    assert!((end[0] - 30.0).abs() < 1e-9, "{end:?}");
  }

  #[test]
  fn evenly_spaced_control_points_give_an_evenly_swept_line() {
    let c =
      Curve::new(line_control(), 3, None, None, EndType::Clamped).unwrap();
    for i in 0..=10 {
      let u = i as f64 / 10.0;
      let p = c.at(u);
      assert!((p[0] - 30.0 * u).abs() < 1e-6, "at {u}: {p:?}");
    }
  }

  #[test]
  fn the_basis_functions_always_sum_to_one() {
    let c =
      Curve::new(line_control(), 2, None, None, EndType::Clamped).unwrap();
    for i in 0..=20 {
      let t = c.span.0 + (c.span.1 - c.span.0) * i as f64 / 20.0;
      let total: f64 = (0..c.control.len())
        .map(|k| basis(k, c.degree, t, &c.knots))
        .sum();
      assert!((total - 1.0).abs() < 1e-9, "at {t}: {total}");
    }
  }

  #[test]
  fn a_heavier_control_point_pulls_the_curve_towards_it() {
    let control = vec![[0.0, 0.0, 0.0], [10.0, 10.0, 0.0], [20.0, 0.0, 0.0]];
    let plain =
      Curve::new(control.clone(), 2, None, None, EndType::Clamped).unwrap();
    let heavy = Curve::new(
      control,
      2,
      Some(vec![1.0, 8.0, 1.0]),
      None,
      EndType::Clamped,
    )
    .unwrap();
    assert!(
      heavy.at(0.5)[1] > plain.at(0.5)[1],
      "weighting the middle point should raise the curve"
    );
  }

  #[test]
  fn too_few_control_points_for_the_degree_is_refused() {
    let control = vec![[0.0; 3], [1.0, 0.0, 0.0]];
    assert!(Curve::new(control, 3, None, None, EndType::Clamped).is_err());
  }

  #[test]
  fn a_weight_of_zero_or_less_is_refused() {
    let e = Curve::new(
      line_control(),
      3,
      Some(vec![1.0, 0.0, 1.0, 1.0]),
      None,
      EndType::Clamped,
    );
    assert!(e.is_err());
  }
}
