//! BOSL2's `transforms.scad`.
//!
//! Each of these is three things at once, exactly as in BOSL2: called with no
//! target it returns the 4×4 matrix, called with a list of points it moves
//! them, and called with a shape it transforms the shape.
//!
//! ```lua
//! local m     = bosl.up(10)              -- the matrix
//! local moved = bosl.up(10, {{0,0,0}})   -- the points, moved
//! local solid = bosl.up(10, cube(5))     -- the shape, moved
//! ```

use mlua::{Lua, Result as LuaResult, Value as LuaValue};

use crate::bosl::attach::transform as wrap_transform;
use crate::bosl::value::{Args, Val};
use crate::bosl::vecmath::{Mat4, V3, vector_angle, vector_axis};
use crate::geometry::{CsgGeometry, CsgSketch};
use crate::scad_export::ScadNode;

/// What a transform was asked to act on.
enum Target {
  /// Nothing: the caller wants the matrix itself.
  Matrix,
  Points(Val),
  Geometry(CsgGeometry),
  Sketch(CsgSketch),
}

fn target_of(a: &Args) -> Target {
  // `apply()` calls its target `points`; everything else calls it `p`.
  match a.raw("p").or_else(|| a.raw("points")) {
    None => Target::Matrix,
    Some(LuaValue::UserData(ud)) => {
      if let Ok(g) = ud.borrow::<CsgGeometry>() {
        Target::Geometry(g.clone())
      } else if let Ok(s) = ud.borrow::<CsgSketch>() {
        Target::Sketch(s.clone())
      } else {
        Target::Matrix
      }
    }
    Some(v) => match Val::from_lua(v) {
      Some(val) => Target::Points(val),
      None => Target::Matrix,
    },
  }
}

/// Hand back the matrix, the moved points, or the transformed shape.
fn apply_to(
  lua: &Lua,
  a: &Args,
  m: Mat4,
  scad_args: String,
) -> LuaResult<LuaValue> {
  match target_of(a) {
    Target::Matrix => {
      Val::list((0..4).map(|r| Val::vec((0..4).map(|c| m.0[r * 4 + c]))))
        .to_lua(lua)
    }

    Target::Points(val) => match move_points(&val, &m) {
      Some(v) => v.to_lua(lua),
      None => a.err("p must be a point, a list of points, or a shape"),
    },

    Target::Geometry(g) => {
      let child = g.scad.clone().unwrap_or(ScadNode::Union(vec![]));
      let node = bosl_wrapper(a.func(), scad_args, child.clone(), m);
      Ok(LuaValue::UserData(lua.create_userdata(CsgGeometry {
        name: g.name.clone(),
        mesh: None,
        color: g.color,
        material: g.material,
        scad: Some(node),
      })?))
    }

    Target::Sketch(s) => {
      let child = s.scad.clone().unwrap_or(ScadNode::Union(vec![]));
      let node = bosl_wrapper(a.func(), scad_args, child.clone(), m);
      Ok(LuaValue::UserData(lua.create_userdata(CsgSketch {
        #[cfg(feature = "csgrs")]
        sketch: crate::geometry::empty_sketch(),
        #[cfg(not(feature = "csgrs"))]
        sketch: (),
        color: s.color,
        material: s.material,
        scad: Some(node),
      })?))
    }
  }
}

/// The BOSL2 call, recorded for `.scad` export, carrying the plain transform
/// that every other backend renders.
fn bosl_wrapper(
  function: &'static str,
  args: String,
  child: ScadNode,
  m: Mat4,
) -> ScadNode {
  crate::bosl::bosl_node_with_children(
    "std.scad",
    function,
    args,
    vec![child.clone()],
    Some(wrap_transform(child, m)),
  )
}

/// Move a point, or every point of a path, through the matrix.
fn move_points(val: &Val, m: &Mat4) -> Option<Val> {
  if let Some(p) = val.as_vec() {
    if p.len() == 2 {
      let q = m.apply([p[0], p[1], 0.0]);
      return Some(Val::vec([q[0], q[1]]));
    }
    if p.len() >= 3 {
      let q = m.apply([p[0], p[1], p[2]]);
      return Some(Val::vec(q));
    }
    return None;
  }
  let path = val.as_matrix()?;
  Some(Val::list(path.iter().map(|p| {
    if p.len() == 2 {
      let q = m.apply([p[0], p[1], 0.0]);
      Val::vec([q[0], q[1]])
    } else {
      let q = m.apply(crate::bosl::value::v3(p));
      Val::vec(q)
    }
  })))
}

/// Format the arguments back out for the `.scad` export.
fn fmt_args(parts: &[String]) -> String {
  parts.join(", ")
}

fn fmt_num(v: f64) -> String {
  let s = format!("{v:.6}");
  let s = s.trim_end_matches('0').trim_end_matches('.');
  s.to_string()
}

fn fmt_vec(v: V3) -> String {
  format!("[{}, {}, {}]", fmt_num(v[0]), fmt_num(v[1]), fmt_num(v[2]))
}

// ---------------------------------------------------------------------------
// Translation
// ---------------------------------------------------------------------------

fn build_move(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let v = match a.val("v") {
    Some(Val::Num(n)) => [n, 0.0, 0.0],
    Some(other) => match other.as_vec() {
      Some(p) => crate::bosl::value::v3(&p),
      None => return a.err("v must be a vector"),
    },
    None => [0.0; 3],
  };
  apply_to(lua, a, Mat4::translate(v), fmt_vec(v))
}

/// The single-axis moves, which differ only in which component they set and
/// which way it points.
fn axis_move(
  axis: usize,
  sign: f64,
  param: &'static str,
) -> impl Fn(&Lua, &Args) -> LuaResult<LuaValue> {
  move |lua, a| {
    let given = a.num_or(param, 0.0);
    let mut v = [0.0; 3];
    v[axis] = given * sign;
    apply_to(lua, a, Mat4::translate(v), fmt_num(given))
  }
}

// ---------------------------------------------------------------------------
// Rotation
// ---------------------------------------------------------------------------

/// Build a rotation from the several ways BOSL2 lets one be described.
fn rotation_matrix(a: &Args) -> LuaResult<(Mat4, String)> {
  let cp = a
    .val("cp")
    .and_then(|v| v.as_vec())
    .map(|p| crate::bosl::value::v3(&p));
  let reverse = a.bool_or("reverse", false);

  // `from`/`to` name a rotation by the directions it carries between.
  let from = a
    .val("from")
    .and_then(|v| v.as_vec())
    .map(|p| crate::bosl::value::v3(&p));
  let to = a
    .val("to")
    .and_then(|v| v.as_vec())
    .map(|p| crate::bosl::value::v3(&p));

  let (base, desc) = match (from, to) {
    (Some(f), Some(t)) => {
      // An extra spin about `from` is allowed alongside.
      let spin = a.num_or("a", 0.0);
      let m = Mat4::rot_from_to(f, t).mul(&Mat4::rot_by_axis(f, spin));
      (m, format!("from = {}, to = {}", fmt_vec(f), fmt_vec(t)))
    }
    _ => match a.val("v").and_then(|v| v.as_vec()) {
      // An explicit axis rotates about that axis.
      Some(axis) => {
        let axis = crate::bosl::value::v3(&axis);
        let ang = a.num_or("a", 0.0);
        (
          Mat4::rot_by_axis(axis, ang),
          format!("{}, v = {}", fmt_num(ang), fmt_vec(axis)),
        )
      }
      None => match a.val("a") {
        // A bare number turns about Z; a vector is an X-then-Y-then-Z turn.
        Some(Val::Num(ang)) => (Mat4::zrot(ang), fmt_num(ang)),
        Some(other) => match other.as_vec() {
          Some(v) => {
            let e = crate::bosl::value::v3(&v);
            (
              Mat4::zrot(e[2])
                .mul(&Mat4::yrot(e[1]))
                .mul(&Mat4::xrot(e[0])),
              fmt_vec(e),
            )
          }
          None => return a.err("a must be an angle or a vector of angles"),
        },
        None => (Mat4::identity(), "0".to_string()),
      },
    },
  };

  let m = if reverse { invert_rigid(&base) } else { base };
  // A centre of rotation shifts the pivot away from the origin.
  let m = match cp {
    Some(c) => Mat4::translate(c)
      .mul(&m)
      .mul(&Mat4::translate([-c[0], -c[1], -c[2]])),
    None => m,
  };
  let desc = match cp {
    Some(c) => format!("{desc}, cp = {}", fmt_vec(c)),
    None => desc,
  };
  Ok((m, desc))
}

/// Invert a rotation-and-translation without elimination: the rotation part
/// is orthogonal, so it transposes.
fn invert_rigid(m: &Mat4) -> Mat4 {
  let mut out = Mat4::identity();
  for r in 0..3 {
    for c in 0..3 {
      out.0[r * 4 + c] = m.0[c * 4 + r];
    }
  }
  for r in 0..3 {
    out.0[r * 4 + 3] = -(0..3)
      .map(|k| out.0[r * 4 + k] * m.0[k * 4 + 3])
      .sum::<f64>();
  }
  out
}

fn build_rot(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let (m, desc) = rotation_matrix(a)?;
  apply_to(lua, a, m, desc)
}

/// The single-axis rotations.
fn axis_rot(axis: usize) -> impl Fn(&Lua, &Args) -> LuaResult<LuaValue> {
  move |lua, a| {
    let ang = a.num_or("a", 0.0);
    let base = match axis {
      0 => Mat4::xrot(ang),
      1 => Mat4::yrot(ang),
      _ => Mat4::zrot(ang),
    };
    let m = match a.val("cp").and_then(|v| v.as_vec()) {
      Some(c) => {
        let c = crate::bosl::value::v3(&c);
        Mat4::translate(c)
          .mul(&base)
          .mul(&Mat4::translate([-c[0], -c[1], -c[2]]))
      }
      None => base,
    };
    apply_to(lua, a, m, fmt_num(ang))
  }
}

fn build_tilt(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let Some(to) = a
    .val("to")
    .and_then(|v| v.as_vec())
    .map(|p| crate::bosl::value::v3(&p))
  else {
    return a.err("to is required");
  };
  // Tilting always starts from straight up.
  let base = Mat4::rot_from_to([0.0, 0.0, 1.0], to);
  let base = if a.bool_or("reverse", false) {
    invert_rigid(&base)
  } else {
    base
  };
  let m = match a.val("cp").and_then(|v| v.as_vec()) {
    Some(c) => {
      let c = crate::bosl::value::v3(&c);
      Mat4::translate(c)
        .mul(&base)
        .mul(&Mat4::translate([-c[0], -c[1], -c[2]]))
    }
    None => base,
  };
  apply_to(lua, a, m, format!("to = {}", fmt_vec(to)))
}

// ---------------------------------------------------------------------------
// Scaling and mirroring
// ---------------------------------------------------------------------------

fn axis_scale(
  axis: usize,
  param: &'static str,
) -> impl Fn(&Lua, &Args) -> LuaResult<LuaValue> {
  move |lua, a| {
    let k = a.num_or(param, 1.0);
    let mut s = [1.0; 3];
    s[axis] = k;
    let base = Mat4::scale(s);
    let m = match a.val("cp").and_then(|v| v.as_vec()) {
      Some(c) => {
        let c = crate::bosl::value::v3(&c);
        Mat4::translate(c)
          .mul(&base)
          .mul(&Mat4::translate([-c[0], -c[1], -c[2]]))
      }
      None => base,
    };
    apply_to(lua, a, m, fmt_num(k))
  }
}

/// Scale by a per-axis factor, or by one factor on every axis.
fn build_scale(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let s = match a.val("v") {
    Some(Val::Num(k)) => [k, k, k],
    Some(other) => match other.as_vec() {
      // A short vector leaves the axes it does not mention alone, so
      // scaling a 2D outline does not flatten it.
      Some(p) => {
        let mut out = [1.0; 3];
        for (i, k) in p.iter().take(3).enumerate() {
          out[i] = *k;
        }
        out
      }
      None => return a.err("v must be a number or a vector"),
    },
    None => return a.err("v is required"),
  };
  if s.contains(&0.0) {
    return a.err("a scale factor of zero would flatten the shape away");
  }
  let base = Mat4::scale(s);
  let m = match a.val("cp").and_then(|v| v.as_vec()) {
    Some(c) => {
      let c = crate::bosl::value::v3(&c);
      Mat4::translate(c)
        .mul(&base)
        .mul(&Mat4::translate([-c[0], -c[1], -c[2]]))
    }
    None => base,
  };
  apply_to(lua, a, m, fmt_vec(s))
}

/// Mirror through the plane whose normal is `v`, passing through the origin.
fn build_mirror(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let Some(v) = a.val("v").and_then(|v| v.as_vec()) else {
    return a.err("v is required: the normal of the plane to mirror through");
  };
  let n = crate::bosl::value::v3(&v);
  let len2 = n[0] * n[0] + n[1] * n[1] + n[2] * n[2];
  if len2 < 1e-18 {
    return a.err("v must not be zero");
  }
  // Householder: every point loses twice its component along the normal.
  let mut m = Mat4::identity();
  for r in 0..3 {
    for c in 0..3 {
      m.0[r * 4 + c] = f64::from(r == c) - 2.0 * n[r] * n[c] / len2;
    }
  }
  apply_to(lua, a, m, fmt_vec(n))
}

/// Apply an arbitrary 4×4 matrix.
fn build_multmatrix(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let Some(rows) = a.val("m").and_then(|v| v.as_matrix()) else {
    return a.err("m must be a 4x4 matrix");
  };
  if rows.len() < 3 || rows.iter().any(|r| r.len() < 4) {
    return a.err("m must be a 4x4 matrix");
  }
  let mut out = Mat4::identity();
  for (r, row) in rows.iter().take(4).enumerate() {
    out.0[r * 4..r * 4 + 4].copy_from_slice(&row[..4]);
  }
  apply_to(lua, a, out, String::new())
}

fn axis_flip(
  axis: usize,
  param: &'static str,
) -> impl Fn(&Lua, &Args) -> LuaResult<LuaValue> {
  move |lua, a| {
    // The parameter offsets the mirror plane along the axis it is normal to.
    let offset = a.num_or(param, 0.0);
    let mut s = [1.0; 3];
    s[axis] = -1.0;
    let mut shift = [0.0; 3];
    shift[axis] = offset;
    let m = Mat4::translate(shift)
      .mul(&Mat4::scale(s))
      .mul(&Mat4::translate([-shift[0], -shift[1], -shift[2]]));
    // The flips declare the shape as their first parameter, so the offset
    // has to be named — a bare positional would be read as the shape and
    // rejected by BOSL2's module form.
    apply_to(lua, a, m, format!("{param} = {}", fmt_num(offset)))
  }
}

fn build_skew(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  // Each shear can be given as a slope, or as the angle whose tangent it is.
  let g = |n: &str| match a.num(n) {
    Some(v) => v,
    None => a
      .num(&format!("a{}", &n[1..]))
      .map(|deg| deg.to_radians().tan())
      .unwrap_or(0.0),
  };
  // Each term shears one axis in proportion to another.
  let m = Mat4([
    1.0,
    g("sxy"),
    g("sxz"),
    0.0, //
    g("syx"),
    1.0,
    g("syz"),
    0.0, //
    g("szx"),
    g("szy"),
    1.0,
    0.0, //
    0.0,
    0.0,
    0.0,
    1.0,
  ]);
  let desc = fmt_args(
    &["sxy", "sxz", "syx", "syz", "szx", "szy"]
      .iter()
      .filter(|n| a.has(n))
      .map(|n| format!("{n} = {}", fmt_num(g(n))))
      .collect::<Vec<_>>(),
  );
  apply_to(lua, a, m, desc)
}

/// Build a rotation from two or three axis directions.
fn build_frame_map(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let read = |n: &str| {
    a.val(n)
      .and_then(|v| v.as_vec())
      .map(|p| crate::bosl::value::v3(&p))
      .and_then(crate::bosl::vecmath::unit_or_none)
  };
  let (x, y, z) = (read("x"), read("y"), read("z"));
  // The missing axis follows from the other two, so only two are needed.
  let cross = crate::bosl::vecmath::cross;
  let (x, y, z) = match (x, y, z) {
    (Some(x), Some(y), _) => (x, y, cross(x, y)),
    (Some(x), None, Some(z)) => (x, cross(z, x), z),
    (None, Some(y), Some(z)) => (cross(y, z), y, z),
    (Some(x), None, None) => {
      let z = if x[2].abs() < 0.9 {
        cross(x, [0.0, 0.0, 1.0])
      } else {
        cross(x, [1.0, 0.0, 0.0])
      };
      (x, cross(z, x), z)
    }
    _ => return a.err("give at least two of x, y and z"),
  };
  let m = Mat4([
    x[0], y[0], z[0], 0.0, //
    x[1], y[1], z[1], 0.0, //
    x[2], y[2], z[2], 0.0, //
    0.0, 0.0, 0.0, 1.0,
  ]);
  let m = if a.bool_or("reverse", false) {
    invert_rigid(&m)
  } else {
    m
  };
  apply_to(lua, a, m, String::new())
}

/// Apply an explicit matrix to points or a shape.
fn build_apply(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let rows = a.need_matrix("transform")?;
  let mut m = Mat4::identity();
  for (r, row) in rows.iter().take(4).enumerate() {
    for (c, v) in row.iter().take(4).enumerate() {
      m.0[r * 4 + c] = *v;
    }
  }
  apply_to(lua, a, m, String::new())
}

// ---------------------------------------------------------------------------
// Registration
// ---------------------------------------------------------------------------

/// Register a transform whose implementation is a closure.
fn add(
  lua: &Lua,
  bosl: &mlua::Table,
  name: &'static str,
  params: &'static [&'static str],
  f: impl Fn(&Lua, &Args) -> LuaResult<LuaValue> + 'static,
) -> LuaResult<()> {
  let func = lua.create_function(move |lua, args: mlua::MultiValue| {
    let parsed = Args::parse_pure(name, params, &args)?;
    f(lua, &parsed)
  })?;
  bosl.set(name, func)?;
  Ok(())
}

pub fn register(lua: &Lua, bosl: &mlua::Table) -> LuaResult<()> {
  // BOSL2 also ships the OpenSCAD-spelled transforms, which differ from the
  // built-ins by taking `p` — so they transform a point list as readily as a
  // shape — and by centring on `cp` where that makes sense.
  add(
    lua,
    bosl,
    "rotate",
    &["a", "v", "cp", "reverse", "p"],
    build_rot,
  )?;
  add(lua, bosl, "scale", &["v", "cp", "p"], build_scale)?;
  add(lua, bosl, "mirror", &["v", "p"], build_mirror)?;
  add(lua, bosl, "multmatrix", &["m", "p"], build_multmatrix)?;

  add(lua, bosl, "move", &["v", "p"], build_move)?;
  add(lua, bosl, "translate", &["v", "p"], build_move)?;
  // Each axis move names its parameter after the axis it acts on.
  const X_P: &[&str] = &["x", "p"];
  const Y_P: &[&str] = &["y", "p"];
  const Z_P: &[&str] = &["z", "p"];
  for (name, axis, sign, param, params) in [
    ("left", 0usize, -1.0f64, "x", X_P),
    ("right", 0, 1.0, "x", X_P),
    ("xmove", 0, 1.0, "x", X_P),
    ("fwd", 1, -1.0, "y", Y_P),
    ("back", 1, 1.0, "y", Y_P),
    ("ymove", 1, 1.0, "y", Y_P),
    ("down", 2, -1.0, "z", Z_P),
    ("up", 2, 1.0, "z", Z_P),
    ("zmove", 2, 1.0, "z", Z_P),
  ] {
    add(lua, bosl, name, params, axis_move(axis, sign, param))?;
  }

  add(
    lua,
    bosl,
    "rot",
    &["a", "v", "cp", "from", "to", "reverse", "p"],
    build_rot,
  )?;
  for (name, axis) in [("xrot", 0usize), ("yrot", 1), ("zrot", 2)] {
    add(lua, bosl, name, &["a", "p", "cp"], axis_rot(axis))?;
  }
  add(lua, bosl, "tilt", &["to", "cp", "reverse", "p"], build_tilt)?;

  const X_P_CP: &[&str] = &["x", "p", "cp"];
  const Y_P_CP: &[&str] = &["y", "p", "cp"];
  const Z_P_CP: &[&str] = &["z", "p", "cp"];
  for (name, axis, param, params) in [
    ("xscale", 0usize, "x", X_P_CP),
    ("yscale", 1, "y", Y_P_CP),
    ("zscale", 2, "z", Z_P_CP),
  ] {
    add(lua, bosl, name, params, axis_scale(axis, param))?;
  }
  // The flips take the shape first and the offset second.
  const P_X: &[&str] = &["p", "x"];
  const P_Y: &[&str] = &["p", "y"];
  const P_Z: &[&str] = &["p", "z"];
  for (name, axis, param, params) in [
    ("xflip", 0usize, "x", P_X),
    ("yflip", 1, "y", P_Y),
    ("zflip", 2, "z", P_Z),
  ] {
    add(lua, bosl, name, params, axis_flip(axis, param))?;
  }

  add(
    lua,
    bosl,
    "skew",
    &[
      "p", "sxy", "sxz", "syx", "syz", "szx", "szy", "axy", "axz", "ayx",
      "ayz", "azx", "azy",
    ],
    build_skew,
  )?;
  add(
    lua,
    bosl,
    "frame_map",
    &["x", "y", "z", "p", "reverse"],
    build_frame_map,
  )?;
  add(lua, bosl, "apply", &["transform", "points"], build_apply)?;
  Ok(())
}

/// The angle between two directions, used by the distributors too.
pub fn angle_between(a: V3, b: V3) -> f64 {
  vector_angle(a, b)
}

/// The axis carrying one direction onto another.
pub fn axis_between(a: V3, b: V3) -> V3 {
  vector_axis(a, b)
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
    a.len() == b.len() && a.iter().zip(b).all(|(x, y)| (x - y).abs() < 1e-9)
  }

  #[test]
  fn a_transform_with_no_target_is_its_matrix() {
    let m: Vec<Vec<f64>> = eval("return bosl.up(10)");
    assert_eq!(m.len(), 4);
    assert!(close(&m[2], &[0.0, 0.0, 1.0, 10.0]), "{m:?}");
  }

  #[test]
  fn a_transform_moves_the_points_it_is_given() {
    let p: Vec<Vec<f64>> = eval("return bosl.up(10, {{0,0,0},{1,2,3}})");
    assert!(close(&p[0], &[0.0, 0.0, 10.0]), "{p:?}");
    assert!(close(&p[1], &[1.0, 2.0, 13.0]), "{p:?}");
  }

  #[test]
  fn a_single_point_comes_back_as_a_point() {
    let p: Vec<f64> = eval("return bosl.right(5, {1,2,3})");
    assert!(close(&p, &[6.0, 2.0, 3.0]), "{p:?}");
  }

  #[test]
  fn two_dimensional_points_stay_two_dimensional() {
    let p: Vec<Vec<f64>> = eval("return bosl.right(5, {{1,2}})");
    assert!(close(&p[0], &[6.0, 2.0]), "{p:?}");
  }

  /// Run a script through the full engine and return the one shape it makes.
  fn shape(code: &str) -> crate::scad_export::ScadNode {
    let geoms = crate::lua_engine::execute_lua(code).unwrap();
    geoms[0].scad.clone().unwrap()
  }

  #[test]
  fn a_transform_moves_a_shape_and_the_mesh_follows() {
    let node = shape("render(bosl.up(10, cube(5)))");
    let m = crate::export::materialize_scad_manifold(&node);
    let (lo, hi) = m.bounding_box();
    assert!((lo[2] - 10.0).abs() < 1e-3, "{lo:?}");
    assert!((hi[2] - 15.0).abs() < 1e-3, "{hi:?}");
  }

  #[test]
  fn the_axis_moves_go_the_way_their_names_say() {
    assert!(close(
      &eval::<Vec<f64>>("return bosl.left(3, {0,0,0})"),
      &[-3.0, 0.0, 0.0]
    ));
    assert!(close(
      &eval::<Vec<f64>>("return bosl.fwd(3, {0,0,0})"),
      &[0.0, -3.0, 0.0]
    ));
    assert!(close(
      &eval::<Vec<f64>>("return bosl.back(3, {0,0,0})"),
      &[0.0, 3.0, 0.0]
    ));
    assert!(close(
      &eval::<Vec<f64>>("return bosl.down(3, {0,0,0})"),
      &[0.0, 0.0, -3.0]
    ));
  }

  #[test]
  fn a_bare_angle_turns_about_the_z_axis() {
    let p: Vec<f64> = eval("return bosl.rot({a=90, p={1,0,0}})");
    assert!(close(&p, &[0.0, 1.0, 0.0]), "{p:?}");
    let p: Vec<f64> = eval("return bosl.zrot(90, {1,0,0})");
    assert!(close(&p, &[0.0, 1.0, 0.0]), "{p:?}");
  }

  #[test]
  fn rotation_about_an_explicit_axis() {
    let p: Vec<f64> = eval("return bosl.rot(90, {p={1,0,0}, v={0,1,0}})");
    assert!(close(&p, &[0.0, 0.0, -1.0]), "{p:?}");
  }

  #[test]
  fn rotation_from_one_direction_to_another() {
    let p: Vec<f64> =
      eval("return bosl.rot({p={0,0,1}, from={0,0,1}, to={1,0,0}})");
    assert!(close(&p, &[1.0, 0.0, 0.0]), "{p:?}");
  }

  #[test]
  fn rotation_about_a_centre_leaves_that_point_alone() {
    let p: Vec<f64> = eval("return bosl.zrot(90, {5,5,0}, {5,5,0})");
    assert!(close(&p, &[5.0, 5.0, 0.0]), "{p:?}");
  }

  #[test]
  fn reversing_a_rotation_undoes_it() {
    // `rot`'s second positional is the axis, so the target is named.
    let p: Vec<f64> = eval(
      "local turned = bosl.rot({a=37, p={1,2,3}})
       return bosl.rot({a=37, p=turned, reverse=true})",
    );
    assert!(close(&p, &[1.0, 2.0, 3.0]), "{p:?}");
  }

  #[test]
  fn scaling_and_flipping_act_on_one_axis_each() {
    assert!(close(
      &eval::<Vec<f64>>("return bosl.xscale(2, {3,4,5})"),
      &[6.0, 4.0, 5.0]
    ));
    assert!(close(
      &eval::<Vec<f64>>("return bosl.zflip({3,4,5})"),
      &[3.0, 4.0, -5.0]
    ));
    // An offset mirror plane reflects about that height instead.
    assert!(close(
      &eval::<Vec<f64>>("return bosl.zflip({3,4,5}, 10)"),
      &[3.0, 4.0, 15.0]
    ));
  }

  #[test]
  fn skew_leans_one_axis_along_another() {
    let p: Vec<f64> = eval("return bosl.skew({p={0,0,10}, sxz=0.5})");
    assert!(close(&p, &[5.0, 0.0, 10.0]), "{p:?}");
  }

  #[test]
  fn frame_map_builds_a_rotation_from_its_axes() {
    // Mapping X onto Y and Y onto -X is a quarter turn about Z.
    let p: Vec<f64> =
      eval("return bosl.frame_map({p={1,0,0}, x={0,1,0}, y={-1,0,0}})");
    assert!(close(&p, &[0.0, 1.0, 0.0]), "{p:?}");
  }

  #[test]
  fn apply_uses_the_matrix_it_is_handed() {
    let p: Vec<f64> = eval(
      "return bosl.apply({{1,0,0,5},{0,1,0,0},{0,0,1,0},{0,0,0,1}}, {0,0,0})",
    );
    assert!(close(&p, &[5.0, 0.0, 0.0]), "{p:?}");
    // The matrix a transform returns can be fed straight back in.
    let p: Vec<f64> = eval("return bosl.apply(bosl.up(7), {0,0,0})");
    assert!(close(&p, &[0.0, 0.0, 7.0]), "{p:?}");
  }

  #[test]
  fn scad_export_still_writes_the_bosl_call() {
    let scad = crate::scad_export::generate_scad(&[shape(
      "render(bosl.up(10, cube(5)))",
    )]);
    assert!(scad.contains("up(10)"), "{scad}");
    assert!(scad.contains("cube("), "{scad}");
  }
}
