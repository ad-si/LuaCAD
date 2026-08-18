//! BOSL2's height-mapped surfaces from `shapes3d.scad`.
//!
//! A heightfield is a grid of heights turned into a solid: the grid becomes
//! the top surface, walls drop from its edges, and a flat base closes it.
//! [`cylindrical_heightfield`] does the same thing wrapped round an axis, so
//! the heights run outward from a cylinder rather than upward from a plane.

use mlua::{Lua, Result as LuaResult, Value as LuaValue};

use crate::bosl::value::Args;
use crate::bosl::vnf::Vnf;
use crate::scad_export::ScadNode;

/// Read the grid of heights, which may be given as rows or as a flat list.
fn read_heights(a: &Args, name: &str) -> LuaResult<Vec<Vec<f64>>> {
  let Some(v) = a.val(name) else {
    return a.err(format!("{name} is required"));
  };
  match v.as_matrix() {
    Some(rows) if !rows.is_empty() && rows.iter().all(|r| !r.is_empty()) => {
      let width = rows[0].len();
      if rows.iter().any(|r| r.len() != width) {
        return a.err(format!("every row of {name} must be the same length"));
      }
      Ok(rows)
    }
    _ => a.err(format!("{name} must be a grid of heights")),
  }
}

/// Close a surface grid into a solid by dropping walls to `floor`.
///
/// The grid is the top; the rim walks its four edges once, and the base is
/// the same rim flattened. Winding the base the other way round is what
/// makes the result a solid rather than two surfaces back to back.
fn close_under(top: &[Vec<[f64; 3]>], floor: f64) -> Vnf {
  let rows = top.len();
  let cols = top[0].len();
  let mut points: Vec<[f64; 3]> = top.iter().flatten().copied().collect();
  let at = |r: usize, c: usize| r * cols + c;

  let mut faces: Vec<Vec<usize>> = Vec::new();
  for r in 0..rows - 1 {
    for c in 0..cols - 1 {
      faces.push(vec![at(r, c), at(r, c + 1), at(r + 1, c + 1), at(r + 1, c)]);
    }
  }

  // The rim, walked once anticlockwise seen from above.
  let mut rim: Vec<usize> = Vec::new();
  rim.extend((0..cols).map(|c| at(0, c)));
  rim.extend((1..rows).map(|r| at(r, cols - 1)));
  rim.extend((0..cols - 1).rev().map(|c| at(rows - 1, c)));
  rim.extend((1..rows - 1).rev().map(|r| at(r, 0)));

  let base_start = points.len();
  for i in &rim {
    let p = points[*i];
    points.push([p[0], p[1], floor]);
  }
  for k in 0..rim.len() {
    let n = rim.len();
    faces.push(vec![
      rim[k],
      base_start + k,
      base_start + (k + 1) % n,
      rim[(k + 1) % n],
    ]);
  }
  // The rim runs anticlockwise seen from above, which is the way the top
  // faces, so the base has to be wound against it to face down.
  faces.push((0..rim.len()).rev().map(|k| base_start + k).collect());
  Vnf { points, faces }
}

/// A solid whose top surface follows a grid of heights.
fn heightfield(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let data = read_heights(a, "data")?;
  let size = a.vec2("size").unwrap_or([100.0, 100.0]);
  let bottom = a.num_or("bottom", -20.0);
  let maxz = a.num_or("maxz", 100.0);
  let rows = data.len();
  let cols = data[0].len();
  if rows < 2 || cols < 2 {
    return a.err("data must be at least 2 by 2");
  }
  if bottom >= maxz {
    return a.err("bottom must be below maxz");
  }

  let grid: Vec<Vec<[f64; 3]>> = (0..rows)
    .map(|y| {
      (0..cols)
        .map(|x| {
          [
            size[0] * (x as f64 / (cols - 1) as f64 - 0.5),
            size[1] * (y as f64 / (rows - 1) as f64 - 0.5),
            // Kept a hair above the floor so the walls never collapse.
            data[y][x].clamp(bottom + 0.1, maxz),
          ]
        })
        .collect()
    })
    .collect();
  as_geometry(lua, "heightfield", a, close_under(&grid, bottom).to_node())
}

/// A cylinder whose surface follows a grid of heights.
///
/// The grid's columns run round the cylinder and its rows along the axis, so
/// a height of zero sits on the nominal surface and larger ones stand out
/// from it. `base` is how much solid wall is kept underneath, so a dip in
/// the data cannot cut through to the inside.
fn cylindrical_heightfield(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let data = read_heights(a, "data")?;
  let transpose = a.bool_or("transpose", false);
  let data = if transpose {
    let rows = data.len();
    let cols = data[0].len();
    (0..cols)
      .map(|c| (0..rows).map(|r| data[r][c]).collect())
      .collect()
  } else {
    data
  };
  let rows = data.len();
  let cols = data[0].len();
  if rows < 2 || cols < 2 {
    return a.err("data must be at least 2 by 2");
  }

  let l = a
    .num("l")
    .or_else(|| a.num("h"))
    .or_else(|| a.num("height"))
    .or_else(|| a.num("length"));
  let Some(l) = l else {
    return a.err("a length is required");
  };
  let r = a.radius("r", "d", None);
  let r1 = a.radius("r1", "d1", None).or(r);
  let r2 = a.radius("r2", "d2", None).or(r);
  let (Some(r1), Some(r2)) = (r1, r2) else {
    return a.err("a radius is required");
  };
  let base = a.num_or("base", 1.0);
  let maxh = a.num_or("maxh", 99.0);
  if base <= 0.0 {
    return a.err("base must be positive");
  }

  // Two shells: the textured outside, and a plain inside set `base` in.
  let mut outer: Vec<Vec<[f64; 3]>> = Vec::with_capacity(rows);
  let mut inner: Vec<Vec<[f64; 3]>> = Vec::with_capacity(rows);
  for (y, row) in data.iter().enumerate() {
    let v = y as f64 / (rows - 1) as f64;
    let z = (v - 0.5) * l;
    let nominal = r1 + (r2 - r1) * v;
    let mut ring_out = Vec::with_capacity(cols);
    let mut ring_in = Vec::with_capacity(cols);
    for (x, height) in row.iter().enumerate() {
      let ang = 2.0 * std::f64::consts::PI * x as f64 / cols as f64;
      let h = height.min(maxh).max(-(nominal - base));
      let (s, c) = ang.sin_cos();
      ring_out.push([(nominal + h) * c, (nominal + h) * s, z]);
      ring_in.push([(nominal - base) * c, (nominal - base) * s, z]);
    }
    outer.push(ring_out);
    inner.push(ring_in);
  }

  // Outside going up, inside coming back down, so the two ends close over.
  let mut grid = outer;
  grid.extend(inner.into_iter().rev());
  let vnf = Vnf::vertex_array(&grid, crate::bosl::vnf::Caps::NONE, true, true);
  as_geometry(lua, "cylindrical_heightfield", a, vnf.to_node())
}

/// A ruler, for checking a model's scale against a printed one.
fn ruler(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let length = a.num_or("length", 100.0);
  let depth = a.num_or("depth", 3.0);
  let thickness = a.num_or("thickness", 1.0);
  let unit = a.num_or("unit", 1.0);
  let inch = a.bool_or("inch", false);
  let unit = if inch { unit * 25.4 } else { unit };
  if length <= 0.0 || unit <= 0.0 {
    return a.err("length and unit must be positive");
  }
  let width = a.num_or("width", depth * 3.0);

  // A backing strip, with a pip at every unit and a longer one every fifth
  // and tenth, which is what makes a scale readable at a glance.
  let mut parts = vec![ScadNode::Cube {
    w: length as f32,
    d: width as f32,
    h: thickness as f32,
    center: false,
  }];
  let count = (length / unit).floor() as i64;
  for i in 0..=count {
    let x = i as f64 * unit;
    let long = if i % 10 == 0 {
      1.0
    } else if i % 5 == 0 {
      0.66
    } else {
      0.33
    };
    parts.push(ScadNode::Translate {
      x: (x - unit / 20.0) as f32,
      y: (width - width * long) as f32,
      z: thickness as f32,
      child: Box::new(ScadNode::Cube {
        w: (unit / 10.0) as f32,
        d: (width * long) as f32,
        h: (thickness / 2.0) as f32,
        center: false,
      }),
    });
  }
  as_geometry(lua, "ruler", a, ScadNode::Union(parts))
}

fn as_geometry(
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
  use crate::bosl::value::register_pure;
  register_pure(
    lua,
    bosl,
    "heightfield",
    &[
      "data",
      "size",
      "bottom",
      "maxz",
      "xrange",
      "yrange",
      "style",
      "convexity",
    ],
    heightfield,
  )?;
  register_pure(
    lua,
    bosl,
    "cylindrical_heightfield",
    &[
      "data",
      "l",
      "r",
      "base",
      "transpose",
      "aspect",
      "style",
      "maxh",
      "xrange",
      "yrange",
      "r1",
      "r2",
      "d",
      "d1",
      "d2",
      "h",
      "height",
      "length",
      "convexity",
    ],
    cylindrical_heightfield,
  )?;
  register_pure(
    lua,
    bosl,
    "ruler",
    &[
      "length",
      "width",
      "thickness",
      "depth",
      "labels",
      "pipscale",
      "maxscale",
      "colors",
      "alpha",
      "unit",
      "inch",
    ],
    ruler,
  )?;
  Ok(())
}

#[cfg(test)]
mod tests {
  use super::*;

  fn grid(rows: usize, cols: usize, h: f64) -> Vec<Vec<[f64; 3]>> {
    (0..rows)
      .map(|y| (0..cols).map(|x| [x as f64, y as f64, h]).collect())
      .collect()
  }

  /// The volume a closed mesh encloses, for checking the solid came out
  /// the right way round.
  fn volume(v: &Vnf) -> f64 {
    let mut total = 0.0;
    for face in &v.faces {
      for j in 1..face.len() - 1 {
        let (p0, p1, p2) =
          (v.points[face[0]], v.points[face[j]], v.points[face[j + 1]]);
        total += p0[0] * (p1[1] * p2[2] - p1[2] * p2[1])
          - p0[1] * (p1[0] * p2[2] - p1[2] * p2[0])
          + p0[2] * (p1[0] * p2[1] - p1[1] * p2[0]);
      }
    }
    total / 6.0
  }

  #[test]
  fn a_flat_field_closes_into_a_box_of_the_right_volume() {
    // A 3x3 grid one unit apart, held at z = 2, floored at z = 0.
    let v = close_under(&grid(3, 3, 2.0), 0.0);
    assert!(
      (volume(&v).abs() - 2.0 * 2.0 * 2.0).abs() < 1e-9,
      "{}",
      volume(&v)
    );
  }

  #[test]
  fn the_solid_is_wound_so_it_encloses_a_positive_volume() {
    let v = close_under(&grid(4, 5, 3.0), -1.0);
    assert!(volume(&v) > 0.0, "{}", volume(&v));
  }

  #[test]
  fn every_face_indexes_a_point_that_exists() {
    let v = close_under(&grid(4, 6, 1.0), 0.0);
    for f in &v.faces {
      assert!(f.len() >= 3);
      for i in f {
        assert!(*i < v.points.len());
      }
    }
  }
}
