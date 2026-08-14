//! BOSL2's `coords.scad`: changing the dimension of points and converting
//! between coordinate systems.
//!
//! Angles are in degrees, and the spherical convention is OpenSCAD's:
//! `theta` measured round the Z axis from +X, `phi` down from +Z.

use mlua::{Lua, Result as LuaResult, Value as LuaValue};

use crate::bosl::value::{Args, PureFn, Val, num_list, register_all};

const EPS: f64 = 1e-9;

/// Resize a point to `n` components, padding with zeros or dropping the tail.
fn resize(p: &[f64], n: usize) -> Vec<f64> {
  (0..n).map(|i| p.get(i).copied().unwrap_or(0.0)).collect()
}

/// A point or, if given a list of them, every point in it.
fn point_or_path(
  lua: &Lua,
  a: &Args,
  name: &str,
  n: usize,
) -> LuaResult<LuaValue> {
  let v = a.need_val(name)?;
  if let Some(p) = v.as_vec() {
    return num_list(lua, &resize(&p, n));
  }
  let Some(path) = v.as_matrix() else {
    return a.err(format!("{name} must be a point or a list of points"));
  };
  Val::list(path.iter().map(|p| Val::vec(resize(p, n)))).to_lua(lua)
}

fn point2d(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  point_or_path(lua, a, "p", 2)
}

fn point3d(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  point_or_path(lua, a, "p", 3)
}

fn point4d(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  point_or_path(lua, a, "p", 4)
}

/// Resize every point of a path, filling the new component with `fill`.
fn path_to(lua: &Lua, a: &Args, n: usize) -> LuaResult<LuaValue> {
  let path = a.need_matrix("points")?;
  let fill = a.num_or("fill", 0.0);
  Val::list(
    path
      .iter()
      .map(|p| Val::vec((0..n).map(|i| p.get(i).copied().unwrap_or(fill)))),
  )
  .to_lua(lua)
}

fn path2d(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  // Dropping to 2D always discards the extra components rather than filling.
  let path = a.need_matrix("points")?;
  Val::list(path.iter().map(|p| Val::vec(resize(p, 2)))).to_lua(lua)
}

fn path3d(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  path_to(lua, a, 3)
}

fn path4d(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  path_to(lua, a, 4)
}

fn polar_to_xy(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  // Either a radius and an angle, or one [r, theta] pair.
  let (r, theta) = match (a.val("r"), a.num("theta")) {
    (Some(Val::Num(r)), Some(t)) => (r, t),
    (Some(v), None) => {
      let v = v.as_vec().unwrap_or_default();
      if v.len() != 2 {
        return a.err("give a radius and an angle, or one [r, theta] pair");
      }
      (v[0], v[1])
    }
    _ => return a.err("give a radius and an angle, or one [r, theta] pair"),
  };
  let (s, c) = theta.to_radians().sin_cos();
  num_list(lua, &[r * c, r * s])
}

fn xy_to_polar(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let (x, y) = match (a.val("x"), a.num("y")) {
    (Some(Val::Num(x)), Some(y)) => (x, y),
    (Some(v), None) => {
      let v = v.as_vec().unwrap_or_default();
      if v.len() < 2 {
        return a.err("give x and y, or one [x, y] point");
      }
      (v[0], v[1])
    }
    _ => return a.err("give x and y, or one [x, y] point"),
  };
  num_list(lua, &[(x * x + y * y).sqrt(), y.atan2(x).to_degrees()])
}

fn cylindrical_to_xyz(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let (r, theta, z) = triple(a, "r", "theta", "z")?;
  let (s, c) = theta.to_radians().sin_cos();
  num_list(lua, &[r * c, r * s, z])
}

fn xyz_to_cylindrical(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let (x, y, z) = triple(a, "x", "y", "z")?;
  num_list(lua, &[(x * x + y * y).sqrt(), y.atan2(x).to_degrees(), z])
}

fn spherical_to_xyz(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let (r, theta, phi) = triple(a, "r", "theta", "phi")?;
  let (st, ct) = theta.to_radians().sin_cos();
  let (sp, cp) = phi.to_radians().sin_cos();
  num_list(lua, &[r * sp * ct, r * sp * st, r * cp])
}

fn xyz_to_spherical(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let (x, y, z) = triple(a, "x", "y", "z")?;
  let r = (x * x + y * y + z * z).sqrt();
  let theta = y.atan2(x).to_degrees();
  // Guard the pole, where the direction round the axis is undefined.
  let phi = if r < EPS {
    0.0
  } else {
    (z / r).clamp(-1.0, 1.0).acos().to_degrees()
  };
  num_list(lua, &[r, theta, phi])
}

fn altaz_to_xyz(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let (alt, az, r) = triple(a, "alt", "az", "r")?;
  // Azimuth is measured clockwise from +Y, not counter-clockwise from +X.
  let (sa, ca) = alt.to_radians().sin_cos();
  let (sz, cz) = az.to_radians().sin_cos();
  num_list(lua, &[r * ca * sz, r * ca * cz, r * sa])
}

fn xyz_to_altaz(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let (x, y, z) = triple(a, "x", "y", "z")?;
  let flat = (x * x + y * y).sqrt();
  num_list(
    lua,
    &[
      z.atan2(flat).to_degrees(),
      x.atan2(y).to_degrees(),
      (x * x + y * y + z * z).sqrt(),
    ],
  )
}

/// Read three values given either separately or as one 3-vector.
fn triple(
  a: &Args,
  n1: &str,
  n2: &str,
  n3: &str,
) -> LuaResult<(f64, f64, f64)> {
  match (a.val(n1), a.num(n2), a.num(n3)) {
    (Some(Val::Num(x)), Some(y), Some(z)) => Ok((x, y, z)),
    (Some(v), None, None) => {
      let v = v.as_vec().unwrap_or_default();
      if v.len() < 3 {
        return a.err(format!("give {n1}, {n2} and {n3}, or one 3-vector"));
      }
      Ok((v[0], v[1], v[2]))
    }
    _ => a.err(format!("give {n1}, {n2} and {n3}, or one 3-vector")),
  }
}

/// An orthonormal frame for a plane: its origin, two axes lying in it, and
/// its normal.
type Frame = ([f64; 3], [f64; 3], [f64; 3], [f64; 3]);

fn plane_frame(plane: &[Vec<f64>]) -> Option<Frame> {
  use crate::bosl::value::v3;
  let p0 = v3(plane.first()?);
  let p1 = v3(plane.get(1)?);
  let p2 = v3(plane.get(2)?);
  let e1 = sub3(p1, p0);
  let n = cross3(e1, sub3(p2, p0));
  let u = unit3(e1)?;
  let w = unit3(n)?;
  let v = cross3(w, u);
  Some((p0, u, v, w))
}

fn sub3(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
  [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn cross3(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
  [
    a[1] * b[2] - a[2] * b[1],
    a[2] * b[0] - a[0] * b[2],
    a[0] * b[1] - a[1] * b[0],
  ]
}

fn dot3(a: [f64; 3], b: [f64; 3]) -> f64 {
  a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn unit3(v: [f64; 3]) -> Option<[f64; 3]> {
  let n = dot3(v, v).sqrt();
  if n < EPS {
    None
  } else {
    Some([v[0] / n, v[1] / n, v[2] / n])
  }
}

/// Flatten 3D points that lie on a plane into that plane's own 2D frame.
fn project_plane(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let plane = a.need_matrix("plane")?;
  let Some((origin, u, v, _)) = plane_frame(&plane) else {
    return a.err("plane must be three points that are not collinear");
  };
  let target = a.need_val("p")?;
  let flatten = |p: &[f64]| -> Val {
    let d = sub3(crate::bosl::value::v3(p), origin);
    Val::vec([dot3(d, u), dot3(d, v)])
  };
  match target.as_vec() {
    Some(p) if p.len() >= 2 => flatten(&p).to_lua(lua),
    _ => {
      let Some(points) = target.as_matrix() else {
        return a.err("p must be a point or a list of points");
      };
      Val::list(points.iter().map(|p| flatten(p))).to_lua(lua)
    }
  }
}

/// The inverse of [`project_plane`]: 2D points back onto the plane in 3D.
fn lift_plane(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let plane = a.need_matrix("plane")?;
  let Some((origin, u, v, _)) = plane_frame(&plane) else {
    return a.err("plane must be three points that are not collinear");
  };
  let target = a.need_val("p")?;
  let lift = |p: &[f64]| -> Val {
    let (x, y) = (
      p.first().copied().unwrap_or(0.0),
      p.get(1).copied().unwrap_or(0.0),
    );
    Val::vec([
      origin[0] + u[0] * x + v[0] * y,
      origin[1] + u[1] * x + v[1] * y,
      origin[2] + u[2] * x + v[2] * y,
    ])
  };
  match target.as_vec() {
    Some(p) if p.len() >= 2 => lift(&p).to_lua(lua),
    _ => {
      let Some(points) = target.as_matrix() else {
        return a.err("p must be a point or a list of points");
      };
      Val::list(points.iter().map(|p| lift(p))).to_lua(lua)
    }
  }
}

pub fn register(lua: &Lua, bosl: &mlua::Table) -> LuaResult<()> {
  register_all(
    lua,
    bosl,
    &[
      ("point2d", &["p", "fill"], point2d as PureFn),
      ("point3d", &["p", "fill"], point3d),
      ("point4d", &["p", "fill"], point4d),
      ("path2d", &["points"], path2d),
      ("path3d", &["points", "fill"], path3d),
      ("path4d", &["points", "fill"], path4d),
      ("polar_to_xy", &["r", "theta"], polar_to_xy),
      ("xy_to_polar", &["x", "y"], xy_to_polar),
      (
        "cylindrical_to_xyz",
        &["r", "theta", "z"],
        cylindrical_to_xyz,
      ),
      ("xyz_to_cylindrical", &["x", "y", "z"], xyz_to_cylindrical),
      ("spherical_to_xyz", &["r", "theta", "phi"], spherical_to_xyz),
      ("xyz_to_spherical", &["x", "y", "z"], xyz_to_spherical),
      ("altaz_to_xyz", &["alt", "az", "r"], altaz_to_xyz),
      ("xyz_to_altaz", &["x", "y", "z"], xyz_to_altaz),
      ("project_plane", &["plane", "p"], project_plane),
      ("lift_plane", &["plane", "p"], lift_plane),
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
    a.len() == b.len() && a.iter().zip(b).all(|(x, y)| (x - y).abs() < 1e-9)
  }

  #[test]
  fn changing_dimension_pads_and_truncates() {
    let p: Vec<f64> = eval("return bosl.point3d({1, 2})");
    assert_eq!(p, vec![1.0, 2.0, 0.0]);
    let p: Vec<f64> = eval("return bosl.point2d({1, 2, 3})");
    assert_eq!(p, vec![1.0, 2.0]);
    let p: Vec<f64> = eval("return bosl.point4d({1, 2, 3})");
    assert_eq!(p, vec![1.0, 2.0, 3.0, 0.0]);
  }

  #[test]
  fn a_path_converts_every_point_at_once() {
    let p: Vec<Vec<f64>> = eval("return bosl.path3d({{1,2},{3,4}})");
    assert_eq!(p, vec![vec![1.0, 2.0, 0.0], vec![3.0, 4.0, 0.0]]);
    let p: Vec<Vec<f64>> = eval("return bosl.path3d({{1,2},{3,4}}, 7)");
    assert_eq!(p, vec![vec![1.0, 2.0, 7.0], vec![3.0, 4.0, 7.0]]);
  }

  #[test]
  fn polar_and_cartesian_round_trip() {
    let p: Vec<f64> = eval("return bosl.polar_to_xy(10, 90)");
    assert!(close(&p, &[0.0, 10.0]), "{p:?}");
    let q: Vec<f64> = eval("return bosl.xy_to_polar({0, 10})");
    assert!(close(&q, &[10.0, 90.0]), "{q:?}");
  }

  #[test]
  fn spherical_and_cartesian_round_trip() {
    let p: Vec<f64> = eval("return bosl.spherical_to_xyz(10, 0, 90)");
    assert!(close(&p, &[10.0, 0.0, 0.0]), "{p:?}");
    let p: Vec<f64> = eval("return bosl.spherical_to_xyz(10, 0, 0)");
    assert!(close(&p, &[0.0, 0.0, 10.0]), "{p:?}");
    let q: Vec<f64> = eval("return bosl.xyz_to_spherical({0, 0, 10})");
    assert!(close(&q, &[10.0, 0.0, 0.0]), "{q:?}");
  }

  #[test]
  fn cylindrical_and_cartesian_round_trip() {
    let p: Vec<f64> = eval("return bosl.cylindrical_to_xyz(5, 90, 3)");
    assert!(close(&p, &[0.0, 5.0, 3.0]), "{p:?}");
    let q: Vec<f64> = eval("return bosl.xyz_to_cylindrical({0, 5, 3})");
    assert!(close(&q, &[5.0, 90.0, 3.0]), "{q:?}");
  }

  #[test]
  fn azimuth_is_measured_clockwise_from_the_y_axis() {
    // Due north, level: straight along +Y.
    let p: Vec<f64> = eval("return bosl.altaz_to_xyz(0, 0, 10)");
    assert!(close(&p, &[0.0, 10.0, 0.0]), "{p:?}");
    // Due east.
    let p: Vec<f64> = eval("return bosl.altaz_to_xyz(0, 90, 10)");
    assert!(close(&p, &[10.0, 0.0, 0.0]), "{p:?}");
    // Straight up.
    let p: Vec<f64> = eval("return bosl.altaz_to_xyz(90, 0, 10)");
    assert!(close(&p, &[0.0, 0.0, 10.0]), "{p:?}");
    let q: Vec<f64> = eval("return bosl.xyz_to_altaz({10, 0, 0})");
    assert!(close(&q, &[0.0, 90.0, 10.0]), "{q:?}");
  }

  #[test]
  fn projecting_onto_a_plane_and_lifting_back_returns_the_point() {
    let round: Vec<Vec<f64>> = eval(
      "local plane = {{0,0,5},{1,0,5},{0,1,5}}
       local flat = bosl.project_plane(plane, {{2,3,5},{-1,4,5}})
       return bosl.lift_plane(plane, flat)",
    );
    assert!(close(&round[0], &[2.0, 3.0, 5.0]), "{round:?}");
    assert!(close(&round[1], &[-1.0, 4.0, 5.0]), "{round:?}");
  }

  #[test]
  fn projecting_a_plane_gives_two_dimensional_points() {
    let flat: Vec<f64> =
      eval("return bosl.project_plane({{0,0,0},{1,0,0},{0,1,0}}, {3,4,0})");
    assert!(close(&flat, &[3.0, 4.0]), "{flat:?}");
  }

  #[test]
  fn a_degenerate_plane_is_rejected() {
    let lua = Lua::new();
    register_bosl(&lua).unwrap();
    let err = lua
      .load("return bosl.project_plane({{0,0,0},{1,0,0},{2,0,0}}, {1,1,1})")
      .eval::<mlua::Value>()
      .unwrap_err()
      .to_string();
    assert!(err.contains("collinear"), "{err}");
  }
}
