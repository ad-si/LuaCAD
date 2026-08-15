//! The remaining odds and ends of BOSL2, gathered by what they do rather
//! than which file they came from.
//!
//! Three kinds of thing live here. Anchor and edge *names*, which are plain
//! strings BOSL2 builds with `EDGE()` and `FACE()`. A handful of small
//! computations that never grew a file of their own. And the shapes BOSL2
//! spells the same as OpenSCAD — `cube`, `circle`, `union` — which LuaCAD
//! already has as globals or methods but which a script ported from BOSL2
//! will reach for under `bosl.`.

use mlua::{Lua, Result as LuaResult, Value as LuaValue};

use crate::bosl::value::{Args, PureFn, Val, register_all};

// ---------------------------------------------------------------------------
// Naming edges and faces
// ---------------------------------------------------------------------------

/// The name of a numbered edge, optionally on the top or bottom.
///
/// `EDGE(2)` is `"edge2"`; `EDGE(bosl.TOP, 2)` is `"top_edge2"`. The names go
/// to the `edges` and `except` selectors, which is how a rounding is asked
/// for on one edge rather than all of them.
fn edge_name(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let Some(first) = a.val("a") else {
    return a.err("EDGE needs an edge number");
  };
  let Some(second) = a.val("b") else {
    let Val::Num(i) = first else {
      return a.err("EDGE needs an edge number");
    };
    return lua
      .create_string(format!("edge{}", i as i64))
      .map(LuaValue::String);
  };
  let Val::Num(i) = second else {
    return a.err("EDGE's second argument must be an edge number");
  };
  // The direction may be given as a number or as one of the anchor vectors,
  // whose Z component is what picks the end.
  let dir = match &first {
    Val::Num(n) => *n,
    other => match other.as_vec() {
      Some(v) if v.len() >= 3 => v[2],
      _ => {
        return a
          .err("EDGE's direction must be bosl.TOP, bosl.BOT or bosl.CENTER");
      }
    },
  };
  let prefix = match dir.round() as i64 {
    1 => "top_",
    -1 => "bot_",
    0 => "",
    _ => {
      return a
        .err("EDGE's direction must be bosl.TOP, bosl.BOT or bosl.CENTER");
    }
  };
  lua
    .create_string(format!("{prefix}edge{}", i as i64))
    .map(LuaValue::String)
}

/// The name of a numbered face.
fn face_name(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let i = a.need_num("i")?;
  lua
    .create_string(format!("face{}", i as i64))
    .map(LuaValue::String)
}

// ---------------------------------------------------------------------------
// Small computations
// ---------------------------------------------------------------------------

/// The best fraction approximating `x` with a denominator no larger than
/// `maxq`.
///
/// Found by continued fractions: peel off the whole part, invert what is
/// left, repeat. Stopping as soon as the denominator would overshoot leaves
/// the closest fraction that still fits, which is how a gear ratio or a
/// thread pitch gets a tidy exact form.
fn rational_approx(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let x = a.need_num("x")?;
  let maxq = a.need_num("maxq")?;
  if maxq < 1.0 {
    return a.err("maxq must be at least 1");
  }
  let sign = if x < 0.0 { -1.0 } else { 1.0 };
  let mut value = x.abs();
  // The two most recent convergents, as numerator/denominator pairs.
  let (mut p_prev, mut q_prev) = (1.0f64, 0.0f64);
  let (mut p, mut q) = (value.floor(), 1.0f64);
  let mut best = (p, q);
  loop {
    let frac = value - value.floor();
    if frac.abs() < 1e-12 {
      break;
    }
    value = 1.0 / frac;
    let next = value.floor();
    let (p_new, q_new) = (next * p + p_prev, next * q + q_prev);
    if q_new > maxq {
      break;
    }
    p_prev = p;
    q_prev = q;
    p = p_new;
    q = q_new;
    best = (p, q);
  }
  Val::vec([sign * best.0, best.1]).to_lua(lua)
}

/// Whether every entry of a list has the same shape as the first.
fn is_homogenous(_lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let Some(v) = a.val("l") else {
    return Ok(LuaValue::Boolean(false));
  };
  let Some(items) = v.as_list() else {
    return Ok(LuaValue::Boolean(false));
  };
  let ok = items.is_empty() || items.iter().all(|x| items[0].same_shape(x));
  Ok(LuaValue::Boolean(ok))
}

/// Whether a transform leaves the Z axis alone, so it is really 2D.
///
/// A pure `zscale` is ruled out on purpose: it changes nothing in the plane,
/// so treating it as a 2D transform would silently drop it.
fn is_2d_transform(_lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let Some(rows) = a.val("t").and_then(|v| v.as_matrix()) else {
    return Ok(LuaValue::Boolean(false));
  };
  if rows.len() < 3 || rows.iter().any(|r| r.len() < 4) {
    return Ok(LuaValue::Boolean(false));
  }
  let at = |r: usize, c: usize| rows[r][c];
  let planar = at(2, 0) == 0.0
    && at(2, 1) == 0.0
    && at(2, 3) == 0.0
    && at(0, 2) == 0.0
    && at(1, 2) == 0.0;
  let identity_in_plane =
    at(0, 0) == 1.0 && at(0, 1) == 0.0 && at(1, 0) == 0.0 && at(1, 1) == 1.0;
  Ok(LuaValue::Boolean(
    planar && (at(2, 2) == 1.0 || !identity_in_plane),
  ))
}

/// The squircle radius at an angle, in the Fernández-Guasti form.
fn squircle_radius_fg(_lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let squareness = a.need_num("squareness")?;
  let r = a.need_num("r")?;
  let angle = a.need_num("angle")?;
  let s2a = (squareness * (2.0 * angle).to_radians().sin()).abs();
  let out = if s2a > 0.0 {
    r * 2f64.sqrt() / s2a
      * (1.0 - (1.0 - s2a * s2a).max(0.0).sqrt()).max(0.0).sqrt()
  } else {
    r
  };
  Ok(LuaValue::Number(out))
}

/// The squircle radius at an angle, in the superellipse form.
fn squircle_radius_se(_lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let n = a.need_num("n")?;
  let r = a.need_num("r")?;
  let angle = a.need_num("angle")?;
  let x = angle.to_radians().cos().abs();
  let y = angle.to_radians().sin().abs();
  Ok(LuaValue::Number((x.powf(n) + y.powf(n)).powf(1.0 / n) / r))
}

/// The convex hull of a point list, as a solid.
///
/// A 2D list gives the flat outline; a 3D one gives the enclosing
/// polyhedron.
fn hull_points(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let Some(points) = a.points3("points") else {
    return a.err("points must be a list of points");
  };
  if points.len() < 3 {
    return a.err("at least three points are needed");
  }
  let flat = points.iter().all(|p| p[2].abs() < 1e-12);
  let node = if flat {
    let hull = crate::bosl::regions::convex_hull2(
      &points.iter().map(|p| [p[0], p[1]]).collect::<Vec<_>>(),
    );
    crate::scad_export::ScadNode::Polygon {
      points: hull.iter().map(|p| [p[0] as f32, p[1] as f32]).collect(),
    }
  } else {
    let Some(tris) = crate::bosl::geom::hull3d(&points) else {
      return a.err("the points are all in one plane, so they enclose nothing");
    };
    crate::bosl::vnf::Vnf {
      points,
      faces: tris.iter().map(|t| t.to_vec()).collect(),
    }
    .to_node()
  };
  as_geometry(lua, "hull_points", a, node)
}

/// Wrap a built shape as a BOSL2 call, so `.scad` export still writes it.
fn as_geometry(
  lua: &Lua,
  name: &'static str,
  a: &Args,
  node: crate::scad_export::ScadNode,
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
      scad: Some(scad),
    },
  )?))
}

// ---------------------------------------------------------------------------
// The shapes and operations BOSL2 spells the OpenSCAD way
// ---------------------------------------------------------------------------

use crate::geometry::{CsgGeometry, CsgSketch};
use crate::scad_export::ScadNode;

/// Every shape a call was handed, as nodes.
fn read_shapes(a: &Args, name: &str) -> LuaResult<Vec<ScadNode>> {
  let Some(raw) = a.raw(name) else {
    return a.err(format!("{name} is required"));
  };
  let mut out = Vec::new();
  match raw {
    LuaValue::Table(t) => {
      for i in 1..=t.raw_len() {
        if let Ok(LuaValue::UserData(ud)) = t.get::<LuaValue>(i)
          && let Some(node) = node_of(&ud)
        {
          out.push(node);
        }
      }
    }
    LuaValue::UserData(ud) => {
      if let Some(node) = node_of(ud) {
        out.push(node);
      }
    }
    _ => {}
  }
  if out.is_empty() {
    return a.err(format!("{name} must be a shape or a list of shapes"));
  }
  Ok(out)
}

fn node_of(ud: &mlua::AnyUserData) -> Option<ScadNode> {
  if let Ok(g) = ud.borrow::<CsgGeometry>() {
    return g.scad.clone();
  }
  ud.borrow::<CsgSketch>().ok().and_then(|s| s.scad.clone())
}

fn solid(lua: &Lua, node: ScadNode) -> LuaResult<LuaValue> {
  Ok(LuaValue::UserData(lua.create_userdata(CsgGeometry {
    name: None,
    mesh: None,
    color: None,
    scad: Some(node),
  })?))
}

fn sketch(lua: &Lua, node: ScadNode) -> LuaResult<LuaValue> {
  Ok(LuaValue::UserData(lua.create_userdata(CsgSketch {
    #[cfg(feature = "csgrs")]
    sketch: crate::geometry::empty_sketch(),
    #[cfg(not(feature = "csgrs"))]
    sketch: (),
    color: None,
    scad: Some(node),
  })?))
}

/// A box, sized and optionally centred, as OpenSCAD spells it.
fn cube(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let size = a.sized("size", 3).unwrap_or_else(|| vec![1.0; 3]);
  let center = a.bool_or("center", false);
  solid(
    lua,
    ScadNode::Cube {
      w: size[0] as f32,
      d: size[1] as f32,
      h: size[2] as f32,
      center,
    },
  )
}

fn sphere(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let r = a.radius("r", "d", Some(1.0)).unwrap_or(1.0);
  solid(
    lua,
    ScadNode::Sphere {
      r: r as f32,
      segments: a.segments(r),
    },
  )
}

fn cylinder(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let h = a.num_or("h", 1.0);
  let r = a.radius("r", "d", None);
  let r1 = a.radius("r1", "d1", None).or(r).unwrap_or(1.0);
  let r2 = a.radius("r2", "d2", None).or(r).unwrap_or(r1);
  solid(
    lua,
    ScadNode::Cylinder {
      r1: r1 as f32,
      r2: r2 as f32,
      h: h as f32,
      center: a.bool_or("center", false),
      segments: a.segments(r1.max(r2)),
    },
  )
}

fn circle(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let r = a.radius("r", "d", Some(1.0)).unwrap_or(1.0);
  sketch(
    lua,
    ScadNode::Circle {
      r: r as f32,
      segments: a.segments(r),
    },
  )
}

fn square(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let size = a.sized("size", 2).unwrap_or_else(|| vec![1.0; 2]);
  sketch(
    lua,
    ScadNode::Square {
      w: size[0] as f32,
      h: size[1] as f32,
      center: a.bool_or("center", false),
    },
  )
}

fn text(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let Some(s) = a.string("text") else {
    return a.err("text is required");
  };
  sketch(
    lua,
    ScadNode::Text {
      text: s,
      size: a.num_or("size", 10.0) as f32,
      font: a.string("font").unwrap_or_default(),
      halign: a.string("halign").unwrap_or_else(|| "left".to_string()),
      valign: a.string("valign").unwrap_or_else(|| "baseline".to_string()),
    },
  )
}

/// Combine shapes the way the matching OpenSCAD operation does.
fn combine(name: &'static str) -> impl Fn(&Lua, &Args) -> LuaResult<LuaValue> {
  move |lua, a| {
    let shapes = read_shapes(a, "shapes")?;
    let node = match name {
      "union" => ScadNode::Union(shapes),
      "difference" => ScadNode::Difference(shapes),
      "intersection" => ScadNode::Intersection(shapes),
      _ => ScadNode::Hull(Box::new(ScadNode::Union(shapes))),
    };
    solid(lua, node)
  }
}

/// Move every edge of an outline outward by `r` or `delta`.
///
/// `r` rounds the corners an outward offset opens up, `delta` mitres them,
/// and `chamfer` cuts straight across. This is BOSL2's function form, so it
/// takes and returns a point list rather than a shape.
fn offset(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  use crate::bosl::offset2d::{Corners, JoinStyle};
  let Some(path) = crate::bosl::rounding::read_outline(a, "path") else {
    return a.err("path must be a 2D outline or a sketch");
  };
  if path.len() < 3 {
    return a.err("path must have at least three points");
  }
  let (d, style) = match (a.num("r"), a.num("delta")) {
    (Some(_), Some(_)) => return a.err("give either r or delta, not both"),
    (Some(r), None) => (r, JoinStyle::Round),
    (None, Some(delta)) => (
      delta,
      if a.bool_or("chamfer", false) {
        JoinStyle::Chamfer
      } else {
        JoinStyle::Delta
      },
    ),
    (None, None) => return a.err("r or delta is required"),
  };
  let corners = Corners::plan(&path, style, d.abs(), a.segments(d.abs()));
  if d < 0.0 && !corners.is_valid(d) {
    return a.err(format!(
      "offsetting inward by {} folds the outline over on itself",
      -d
    ));
  }
  Val::list(corners.offset(d, style).iter().map(|p| Val::vec(*p))).to_lua(lua)
}

pub fn register(lua: &Lua, bosl: &mlua::Table) -> LuaResult<()> {
  use crate::bosl::value::register_pure;
  // The OpenSCAD-spelled shapes, which BOSL2 keeps alongside its own.
  register_pure(
    lua,
    bosl,
    "cube",
    &["size", "center", "anchor", "spin", "orient"],
    cube,
  )?;
  register_pure(
    lua,
    bosl,
    "sphere",
    &["r", "d", "anchor", "spin", "orient"],
    sphere,
  )?;
  register_pure(
    lua,
    bosl,
    "cylinder",
    &[
      "h", "r1", "r2", "center", "r", "d", "d1", "d2", "anchor", "spin",
      "orient",
    ],
    cylinder,
  )?;
  register_pure(lua, bosl, "circle", &["r", "d", "anchor", "spin"], circle)?;
  register_pure(
    lua,
    bosl,
    "square",
    &["size", "center", "anchor", "spin"],
    square,
  )?;
  register_pure(
    lua,
    bosl,
    "text",
    &[
      "text",
      "size",
      "font",
      "halign",
      "valign",
      "spacing",
      "direction",
      "language",
      "script",
      "anchor",
      "spin",
    ],
    text,
  )?;
  for name in ["union", "difference", "intersection", "hull"] {
    let f = combine(match name {
      "union" => "union",
      "difference" => "difference",
      "intersection" => "intersection",
      _ => "hull",
    });
    let func = lua.create_function(move |lua, args: mlua::MultiValue| {
      // These take their shapes as a list or as separate arguments, so the
      // whole call is gathered under one name.
      let gathered = lua.create_table()?;
      for (i, v) in args.iter().enumerate() {
        gathered.set(i + 1, v.clone())?;
      }
      let one = mlua::MultiValue::from_iter([LuaValue::Table(gathered)]);
      let parsed = Args::parse_pure("union", &["shapes"], &one)?;
      f(lua, &parsed)
    })?;
    bosl.set(name, func)?;
  }
  register_pure(
    lua,
    bosl,
    "offset",
    &[
      "path",
      "r",
      "delta",
      "chamfer",
      "closed",
      "check_valid",
      "quality",
      "same_length",
    ],
    offset,
  )?;

  register_all(
    lua,
    bosl,
    &[
      ("EDGE", &["a", "b"], edge_name as PureFn),
      ("FACE", &["i"], face_name),
      ("rational_approx", &["x", "maxq"], rational_approx),
      ("is_homogenous", &["l", "depth"], is_homogenous),
      ("is_2d_transform", &["t"], is_2d_transform),
      (
        "squircle_radius_fg",
        &["squareness", "r", "angle"],
        squircle_radius_fg,
      ),
      (
        "squircle_radius_se",
        &["n", "r", "angle"],
        squircle_radius_se,
      ),
      ("hull_points", &["points", "fast"], hull_points),
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
    lua.load(code).eval().unwrap()
  }

  #[test]
  fn an_edge_is_named_by_its_number_and_which_end_it_is_on() {
    assert_eq!(eval::<String>("return bosl.EDGE(2)"), "edge2");
    assert_eq!(eval::<String>("return bosl.EDGE(bosl.TOP, 2)"), "top_edge2");
    assert_eq!(eval::<String>("return bosl.EDGE(bosl.BOT, 0)"), "bot_edge0");
    assert_eq!(eval::<String>("return bosl.FACE(3)"), "face3");
  }

  #[test]
  fn a_rational_approximation_is_the_closest_fraction_that_fits() {
    // Pi to a denominator of at most 10 is 22/7.
    let v: Vec<f64> = eval("return bosl.rational_approx(3.14159265, 10)");
    assert_eq!(v, vec![22.0, 7.0]);
    // With more room, 355/113.
    let v: Vec<f64> = eval("return bosl.rational_approx(3.14159265, 200)");
    assert_eq!(v, vec![355.0, 113.0]);
  }

  #[test]
  fn an_exact_value_comes_back_exactly() {
    let v: Vec<f64> = eval("return bosl.rational_approx(0.75, 100)");
    assert_eq!(v, vec![3.0, 4.0]);
  }

  #[test]
  fn a_list_is_homogenous_when_every_entry_has_the_same_shape() {
    assert!(eval::<bool>("return bosl.is_homogenous({{1,2},{3,4}})"));
    assert!(!eval::<bool>("return bosl.is_homogenous({{1,2},{3,4,5}})"));
  }

  #[test]
  fn a_flat_transform_is_recognised_but_a_z_scale_is_not() {
    let flat = "{{1,0,0,5},{0,1,0,6},{0,0,1,0},{0,0,0,1}}";
    assert!(eval::<bool>(&format!(
      "return bosl.is_2d_transform({flat})"
    )));
    let zscale = "{{1,0,0,0},{0,1,0,0},{0,0,3,0},{0,0,0,1}}";
    assert!(!eval::<bool>(&format!(
      "return bosl.is_2d_transform({zscale})"
    )));
  }

  #[test]
  fn a_squircle_of_no_squareness_is_just_a_circle() {
    let r: f64 = eval("return bosl.squircle_radius_fg(0, 10, 30)");
    assert!((r - 10.0).abs() < 1e-12, "{r}");
  }
}
