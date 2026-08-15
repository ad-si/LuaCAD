//! BOSL2's `distributors.scad`: laying copies of a shape out in a line, on a
//! grid, around an arc, or along a path.
//!
//! Like the transforms, each one is both a module and a function. Given a
//! shape it returns the copies; given a point or path it returns that point
//! moved to each placement; given neither it returns the placements as
//! positions.
//!
//! ```lua
//! bosl.xcopies({spacing = 20, n = 3, p = cube(5)})
//! ```

use mlua::{Lua, Result as LuaResult, Value as LuaValue};

use crate::bosl::attach::transform as wrap_transform;
use crate::bosl::value::{Args, Val, v3};
use crate::bosl::vecmath::{Mat4, V3};
use crate::geometry::{CsgGeometry, CsgSketch};
use crate::scad_export::ScadNode;

/// Hand back the placements, the moved points, or the copied shape.
fn place(
  lua: &Lua,
  a: &Args,
  placements: Vec<Mat4>,
  scad_args: String,
) -> LuaResult<LuaValue> {
  match a.raw("p") {
    // With no target the placements themselves are the answer, as the
    // positions each copy would sit at.
    None => Val::list(
      placements
        .iter()
        .map(|m| Val::vec(m.apply([0.0, 0.0, 0.0]))),
    )
    .to_lua(lua),

    Some(LuaValue::UserData(ud)) => {
      if let Ok(g) = ud.borrow::<CsgGeometry>() {
        let child = g.scad.clone().unwrap_or(ScadNode::Union(vec![]));
        let node = copies_node(a.func(), scad_args, child, &placements);
        return Ok(LuaValue::UserData(lua.create_userdata(CsgGeometry {
          name: g.name.clone(),
          mesh: None,
          color: g.color,
          scad: Some(node),
        })?));
      }
      if let Ok(s) = ud.borrow::<CsgSketch>() {
        let child = s.scad.clone().unwrap_or(ScadNode::Union(vec![]));
        let node = copies_node(a.func(), scad_args, child, &placements);
        return Ok(LuaValue::UserData(lua.create_userdata(CsgSketch {
          #[cfg(feature = "csgrs")]
          sketch: crate::geometry::empty_sketch(),
          #[cfg(not(feature = "csgrs"))]
          sketch: (),
          color: s.color,
          scad: Some(node),
        })?));
      }
      a.err("p must be a shape, a point, or a list of points")
    }

    Some(v) => {
      let Some(val) = Val::from_lua(v) else {
        return a.err("p must be a shape, a point, or a list of points");
      };
      // Each placement produces its own moved copy of the input.
      let mut out = Vec::with_capacity(placements.len());
      for m in &placements {
        match move_points(&val, m) {
          Some(moved) => out.push(moved),
          None => return a.err("p must be a point or a list of points"),
        }
      }
      Val::List(out).to_lua(lua)
    }
  }
}

fn copies_node(
  function: &'static str,
  args: String,
  child: ScadNode,
  placements: &[Mat4],
) -> ScadNode {
  let copies: Vec<ScadNode> = placements
    .iter()
    .map(|m| wrap_transform(child.clone(), *m))
    .collect();
  crate::bosl::bosl_node_with_children(
    "std.scad",
    function,
    args,
    vec![child],
    Some(ScadNode::Union(copies)),
  )
}

fn move_points(val: &Val, m: &Mat4) -> Option<Val> {
  if let Some(p) = val.as_vec() {
    if p.len() == 2 {
      let q = m.apply([p[0], p[1], 0.0]);
      return Some(Val::vec([q[0], q[1]]));
    }
    if p.len() >= 3 {
      return Some(Val::vec(m.apply([p[0], p[1], p[2]])));
    }
    return None;
  }
  let path = val.as_matrix()?;
  Some(Val::list(path.iter().map(|p| {
    if p.len() == 2 {
      let q = m.apply([p[0], p[1], 0.0]);
      Val::vec([q[0], q[1]])
    } else {
      Val::vec(m.apply(v3(p)))
    }
  })))
}

/// Collect the named arguments back into an OpenSCAD argument string.
fn describe(a: &Args, names: &[&str]) -> String {
  names
    .iter()
    .filter(|n| a.has(n))
    .filter_map(|n| {
      let v = a.raw(n)?;
      Some(format!("{n} = {}", crate::bosl::lua_val_to_scad(v)))
    })
    .collect::<Vec<_>>()
    .join(", ")
}

// ---------------------------------------------------------------------------
// Translating copies
// ---------------------------------------------------------------------------

fn move_copies(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let points = a.need_matrix("a")?;
  let placements: Vec<Mat4> =
    points.iter().map(|p| Mat4::translate(v3(p))).collect();
  place(lua, a, placements, describe(a, &["a"]))
}

/// The positions a run of copies occupies along `dir`.
///
/// BOSL2 lets the run be described by any two of the spacing, the count and
/// the overall length; whichever pair is given, the rest follows.
fn line_positions(a: &Args, dir: V3) -> LuaResult<Vec<V3>> {
  let scalar_or_vec = |name: &str| -> Option<V3> {
    match a.val(name) {
      Some(Val::Num(n)) => Some([dir[0] * n, dir[1] * n, dir[2] * n]),
      Some(other) => other.as_vec().map(|v| v3(&v)),
      None => None,
    }
  };
  let l = scalar_or_vec("l");
  let spacing = scalar_or_vec("spacing");
  let n = a.int("n").map(|v| v as usize);
  let p1 = a.val("p1").and_then(|v| v.as_vec()).map(|p| v3(&p));
  let p2 = a.val("p2").and_then(|v| v.as_vec()).map(|p| v3(&p));

  let norm = |v: V3| (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
  let span = match (l, spacing, n, p1, p2) {
    (Some(l), ..) => Some(l),
    (None, Some(s), Some(n), ..) => Some([
      s[0] * (n - 1) as f64,
      s[1] * (n - 1) as f64,
      s[2] * (n - 1) as f64,
    ]),
    (None, _, _, Some(p1), Some(p2)) => {
      Some([p2[0] - p1[0], p2[1] - p1[1], p2[2] - p1[2]])
    }
    _ => None,
  };

  let count = match (n, spacing, span) {
    (Some(n), ..) => n,
    (None, Some(s), Some(span)) if norm(s) > 1e-9 => {
      (norm(span) / norm(s) + 1.000001).floor() as usize
    }
    _ => 2,
  };
  if count == 0 {
    return Ok(vec![]);
  }

  let step = if count <= 1 {
    [0.0; 3]
  } else {
    match (spacing, span) {
      (_, Some(span)) => [
        span[0] / (count - 1) as f64,
        span[1] / (count - 1) as f64,
        span[2] / (count - 1) as f64,
      ],
      (Some(s), None) => s,
      (None, None) => return a.err("give at least one of spacing, l or n"),
    }
  };

  // Without an explicit start the run is centred on the origin.
  let start = match p1 {
    Some(p) => p,
    None => [
      -(count as f64 - 1.0) / 2.0 * step[0],
      -(count as f64 - 1.0) / 2.0 * step[1],
      -(count as f64 - 1.0) / 2.0 * step[2],
    ],
  };
  Ok(
    (0..count)
      .map(|i| {
        [
          start[0] + step[0] * i as f64,
          start[1] + step[1] * i as f64,
          start[2] + step[2] * i as f64,
        ]
      })
      .collect(),
  )
}

/// The single-axis copy runs.
fn axis_copies(dir: V3) -> impl Fn(&Lua, &Args) -> LuaResult<LuaValue> {
  move |lua, a| {
    // A list of spacings places one copy at each offset instead.
    if let Some(Val::List(items)) = a.val("spacing")
      && items.iter().all(|v| v.as_num().is_some())
      && items.len() > 1
    {
      let sp = a
        .val("sp")
        .and_then(|v| v.as_vec())
        .map(|p| v3(&p))
        .unwrap_or([0.0; 3]);
      let placements: Vec<Mat4> = items
        .iter()
        .filter_map(|v| v.as_num())
        .map(|d| {
          Mat4::translate([
            sp[0] + dir[0] * d,
            sp[1] + dir[1] * d,
            sp[2] + dir[2] * d,
          ])
        })
        .collect();
      return place(
        lua,
        a,
        placements,
        describe(a, &["spacing", "n", "l", "sp"]),
      );
    }
    // A scalar `sp` is a distance along the same axis.
    let start = match a.val("sp") {
      Some(Val::Num(d)) => Some([dir[0] * d, dir[1] * d, dir[2] * d]),
      Some(other) => other.as_vec().map(|p| v3(&p)),
      None => None,
    };
    let mut positions = line_positions(a, dir)?;
    // A starting point shifts the whole run rather than centring it.
    if let Some(s) = start {
      positions = positions
        .iter()
        .map(|p| [p[0] + s[0], p[1] + s[1], p[2] + s[2]])
        .collect();
    }
    let placements: Vec<Mat4> =
      positions.iter().map(|p| Mat4::translate(*p)).collect();
    place(
      lua,
      a,
      placements,
      describe(a, &["spacing", "n", "l", "sp"]),
    )
  }
}

fn line_copies(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let positions = line_positions(a, [1.0, 0.0, 0.0])?;
  let placements: Vec<Mat4> =
    positions.iter().map(|p| Mat4::translate(*p)).collect();
  place(
    lua,
    a,
    placements,
    describe(a, &["spacing", "n", "l", "p1", "p2"]),
  )
}

fn grid_copies(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let pair = |name: &str| -> Option<[f64; 2]> {
    match a.val(name) {
      Some(Val::Num(n)) => Some([n, n]),
      Some(other) => other.as_vec().map(|v| {
        [
          v.first().copied().unwrap_or(0.0),
          v.get(1).copied().unwrap_or(0.0),
        ]
      }),
      None => None,
    }
  };
  let spacing = pair("spacing");
  let count = match a.val("n") {
    Some(Val::Num(n)) => Some([n as usize, n as usize]),
    Some(other) => other.as_vec().map(|v| {
      [
        v.first().copied().unwrap_or(1.0) as usize,
        v.get(1).copied().unwrap_or(1.0) as usize,
      ]
    }),
    None => None,
  };
  let size = pair("size");

  let (nx, ny, sx, sy) = match (count, spacing, size) {
    (Some(n), Some(s), _) => (n[0], n[1], s[0], s[1]),
    (Some(n), None, Some(size)) => (
      n[0],
      n[1],
      if n[0] > 1 {
        size[0] / (n[0] - 1) as f64
      } else {
        0.0
      },
      if n[1] > 1 {
        size[1] / (n[1] - 1) as f64
      } else {
        0.0
      },
    ),
    (None, Some(s), Some(size)) => (
      (size[0] / s[0]).floor() as usize + 1,
      (size[1] / s[1]).floor() as usize + 1,
      s[0],
      s[1],
    ),
    _ => return a.err("give the count with either a spacing or a size"),
  };

  let stagger = a.raw("stagger").is_some_and(|v| match v {
    LuaValue::Boolean(b) => *b,
    LuaValue::String(_) => true,
    _ => false,
  });
  let alt = a.string("stagger").as_deref() == Some("alt");

  let mut placements = Vec::with_capacity(nx * ny);
  for j in 0..ny {
    for i in 0..nx {
      // Staggered rows shift by half a step, which packs the copies more
      // tightly than a plain grid.
      let offset = if stagger && (j % 2 == usize::from(alt)) {
        sx / 2.0
      } else {
        0.0
      };
      let x = (i as f64 - (nx as f64 - 1.0) / 2.0) * sx + offset;
      let y = (j as f64 - (ny as f64 - 1.0) / 2.0) * sy;
      placements.push(Mat4::translate([x, y, 0.0]));
    }
  }
  place(
    lua,
    a,
    placements,
    describe(a, &["spacing", "n", "size", "stagger"]),
  )
}

// ---------------------------------------------------------------------------
// Rotating copies
// ---------------------------------------------------------------------------

/// The angles a set of rotated copies sits at.
fn copy_angles(a: &Args) -> Vec<f64> {
  let sa = a.num_or("sa", 0.0);
  match a.int("n") {
    // A count spreads the copies evenly over a full turn.
    Some(n) if n > 0 => {
      (0..n).map(|i| sa + 360.0 * i as f64 / n as f64).collect()
    }
    _ => match a.val("rots") {
      Some(Val::List(items)) => items
        .iter()
        .filter_map(|v| v.as_num())
        .map(|r| r + sa)
        .collect(),
      Some(Val::Num(r)) => vec![r + sa],
      None => vec![],
    },
  }
}

/// Copies turned about one axis, optionally pushed out to a radius first.
fn axis_rot_copies(axis: V3) -> impl Fn(&Lua, &Args) -> LuaResult<LuaValue> {
  move |lua, a| {
    let r = a.radius("r", "d", Some(0.0)).unwrap_or(0.0);
    let cp = a
      .val("cp")
      .and_then(|v| v.as_vec())
      .map(|p| v3(&p))
      .unwrap_or([0.0; 3]);
    let subrot = a.bool_or("subrot", true);
    // The offset direction is perpendicular to the axis of rotation.
    let out = if axis[2].abs() > 0.5 {
      [1.0, 0.0, 0.0]
    } else if axis[0].abs() > 0.5 {
      [0.0, 1.0, 0.0]
    } else {
      [0.0, 0.0, 1.0]
    };

    let placements: Vec<Mat4> = copy_angles(a)
      .iter()
      .map(|ang| {
        let turn = Mat4::rot_by_axis(axis, *ang);
        let offset = Mat4::translate([out[0] * r, out[1] * r, out[2] * r]);
        let m = if subrot {
          turn.mul(&offset)
        } else {
          // Without subrot the copies keep their original orientation and
          // only their positions go round.
          let pos = turn.apply([out[0] * r, out[1] * r, out[2] * r]);
          Mat4::translate(pos)
        };
        Mat4::translate(cp)
          .mul(&m)
          .mul(&Mat4::translate([-cp[0], -cp[1], -cp[2]]))
      })
      .collect();
    place(
      lua,
      a,
      placements,
      describe(a, &["rots", "n", "sa", "r", "d", "cp", "subrot"]),
    )
  }
}

fn rot_copies(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let axis = a
    .val("v")
    .and_then(|v| v.as_vec())
    .map(|p| v3(&p))
    .unwrap_or([0.0, 0.0, 1.0]);
  let cp = a
    .val("cp")
    .and_then(|v| v.as_vec())
    .map(|p| v3(&p))
    .unwrap_or([0.0; 3]);
  let delta = a
    .val("delta")
    .and_then(|v| v.as_vec())
    .map(|p| v3(&p))
    .unwrap_or([0.0; 3]);
  let offset = a.num_or("offset", 0.0);
  let subrot = a.bool_or("subrot", true);

  let mut angles = copy_angles(a);
  for ang in &mut angles {
    *ang += offset;
  }
  let placements: Vec<Mat4> = angles
    .iter()
    .map(|ang| {
      let turn = Mat4::rot_by_axis(axis, *ang);
      let m = if subrot {
        turn.mul(&Mat4::translate(delta))
      } else {
        Mat4::translate(turn.apply(delta))
      };
      Mat4::translate(cp)
        .mul(&m)
        .mul(&Mat4::translate([-cp[0], -cp[1], -cp[2]]))
    })
    .collect();
  place(
    lua,
    a,
    placements,
    describe(
      a,
      &["rots", "v", "cp", "n", "sa", "offset", "delta", "subrot"],
    ),
  )
}

fn arc_copies(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let n = a.int("n").unwrap_or(6).max(1) as usize;
  let rx = a
    .num("rx")
    .or_else(|| a.num("dx").map(|d| d / 2.0))
    .or_else(|| a.radius("r", "d", None))
    .unwrap_or(1.0);
  let ry = a
    .num("ry")
    .or_else(|| a.num("dy").map(|d| d / 2.0))
    .or_else(|| a.radius("r", "d", None))
    .unwrap_or(1.0);
  let sa = a.num_or("sa", 0.0).rem_euclid(360.0);
  let ea = a.num_or("ea", 360.0).rem_euclid(360.0);
  let rot = a.bool_or("rot", true);
  let cp = a
    .val("cp")
    .and_then(|v| v.as_vec())
    .map(|p| v3(&p))
    .unwrap_or([0.0; 3]);

  // A full turn would put the last copy on top of the first, so it gets one
  // extra step and drops the duplicate.
  let count = if (ea - sa).abs() < 0.01 { n + 1 } else { n };
  let sweep = (if ea <= sa { 360.0 } else { 0.0 }) + ea - sa;
  let step = if count > 1 {
    sweep / (count - 1) as f64
  } else {
    0.0
  };

  let placements: Vec<Mat4> = (0..n)
    .map(|i| {
      let ang = sa + i as f64 * step;
      let (s, c) = ang.to_radians().sin_cos();
      let pos = [cp[0] + rx * c, cp[1] + ry * s, cp[2]];
      let spin = if rot {
        (ry * s).atan2(rx * c).to_degrees()
      } else {
        0.0
      };
      Mat4::translate(pos).mul(&Mat4::zrot(spin))
    })
    .collect();
  place(
    lua,
    a,
    placements,
    describe(a, &["n", "r", "rx", "ry", "d", "sa", "ea", "rot", "cp"]),
  )
}

fn sphere_copies(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let n = a.int("n").unwrap_or(100).max(1) as usize;
  let r = a.radius("r", "d", Some(50.0)).unwrap_or(50.0);
  let cone_ang = a.num_or("cone_ang", 90.0);
  let scale = a
    .val("scale")
    .and_then(|v| v.as_vec())
    .map(|p| v3(&p))
    .unwrap_or([1.0; 3]);
  let perp = a.bool_or("perp", true);

  // Points are spread by the golden angle, which spaces them about as
  // evenly over the cap as a spiral can.
  let cnt = ((n as f64) / (cone_ang / 180.0)).ceil() as usize;
  let golden = std::f64::consts::PI * (3.0 - 5f64.sqrt());
  let placements: Vec<Mat4> = (0..n)
    .map(|i| {
      let z = 1.0 - 2.0 * i as f64 / cnt.max(2) as f64;
      let radius = (1.0 - z * z).max(0.0).sqrt();
      let theta = golden * i as f64;
      let dir = [radius * theta.cos(), radius * theta.sin(), z];
      let pos = [
        dir[0] * r * scale[0],
        dir[1] * r * scale[1],
        dir[2] * r * scale[2],
      ];
      let m = Mat4::translate(pos);
      if perp {
        // Each copy is tipped so its up direction points outward.
        m.mul(&Mat4::rot_from_to([0.0, 0.0, 1.0], dir))
      } else {
        m
      }
    })
    .collect();
  place(
    lua,
    a,
    placements,
    describe(a, &["n", "r", "d", "cone_ang", "scale", "perp"]),
  )
}

fn path_copies(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let path = a.need_points3("path")?;
  if path.len() < 2 {
    return a.err("the path needs at least two points");
  }
  let closed = a.bool_or("closed", false);
  let rotate_children = a.bool_or("rotate_children", true);

  // Walk the path measuring as it goes, so copies can be placed by distance
  // rather than by vertex.
  let mut cumulative = vec![0.0];
  let points: Vec<[f64; 3]> = if closed {
    path
      .iter()
      .copied()
      .chain(std::iter::once(path[0]))
      .collect()
  } else {
    path.clone()
  };
  for w in points.windows(2) {
    let d = ((w[1][0] - w[0][0]).powi(2)
      + (w[1][1] - w[0][1]).powi(2)
      + (w[1][2] - w[0][2]).powi(2))
    .sqrt();
    cumulative.push(cumulative.last().unwrap_or(&0.0) + d);
  }
  let total = *cumulative.last().unwrap_or(&0.0);

  let distances: Vec<f64> = match (a.val("dist"), a.int("n"), a.num("spacing"))
  {
    (Some(Val::List(items)), ..) => {
      items.iter().filter_map(|v| v.as_num()).collect()
    }
    (Some(Val::Num(d)), ..) => vec![d],
    (None, Some(n), _) if n > 0 => {
      let n = n as usize;
      let step = if closed || n == 1 {
        total / n as f64
      } else {
        total / (n - 1) as f64
      };
      (0..n).map(|i| i as f64 * step).collect()
    }
    (None, None, Some(spacing)) if spacing > 0.0 => {
      let mut out = Vec::new();
      let mut d = a.num_or("sp", 0.0);
      while d <= total + 1e-9 {
        out.push(d);
        d += spacing;
      }
      out
    }
    // Without a count or spacing, one copy sits at each path vertex.
    _ => cumulative[..path.len()].to_vec(),
  };

  let at = |d: f64| -> (V3, V3) {
    let d = d.clamp(0.0, total);
    let i = cumulative
      .iter()
      .rposition(|c| *c <= d + 1e-12)
      .unwrap_or(0)
      .min(points.len().saturating_sub(2));
    let seg = cumulative[i + 1] - cumulative[i];
    let t = if seg < 1e-12 {
      0.0
    } else {
      (d - cumulative[i]) / seg
    };
    let p = [
      points[i][0] + (points[i + 1][0] - points[i][0]) * t,
      points[i][1] + (points[i + 1][1] - points[i][1]) * t,
      points[i][2] + (points[i + 1][2] - points[i][2]) * t,
    ];
    let tangent = [
      points[i + 1][0] - points[i][0],
      points[i + 1][1] - points[i][1],
      points[i + 1][2] - points[i][2],
    ];
    (p, tangent)
  };

  let placements: Vec<Mat4> = distances
    .iter()
    .map(|d| {
      let (p, tangent) = at(*d);
      let m = Mat4::translate(p);
      if rotate_children {
        // The copies lean along the path, with +X following the tangent.
        m.mul(&Mat4::rot_from_to([1.0, 0.0, 0.0], tangent))
      } else {
        m
      }
    })
    .collect();
  place(
    lua,
    a,
    placements,
    describe(a, &["path", "n", "spacing", "sp", "dist", "closed"]),
  )
}

// ---------------------------------------------------------------------------
// Mirrored copies
// ---------------------------------------------------------------------------

/// Keep the original and add its mirror image.
fn axis_flip_copy(
  axis: usize,
  param: &'static str,
) -> impl Fn(&Lua, &Args) -> LuaResult<LuaValue> {
  move |lua, a| {
    let offset = a.num_or("offset", 0.0);
    let plane = a.num_or(param, 0.0);
    let mut shift = [0.0; 3];
    shift[axis] = offset;
    let mut s = [1.0; 3];
    s[axis] = -1.0;
    let mut at = [0.0; 3];
    at[axis] = plane;

    let mirror = Mat4::translate(at)
      .mul(&Mat4::scale(s))
      .mul(&Mat4::translate([-at[0], -at[1], -at[2]]));
    let placements =
      vec![Mat4::translate(shift), mirror.mul(&Mat4::translate(shift))];
    place(lua, a, placements, describe(a, &["offset", param]))
  }
}

fn mirror_copy(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let v = a
    .val("v")
    .and_then(|v| v.as_vec())
    .map(|p| v3(&p))
    .unwrap_or([0.0, 0.0, 1.0]);
  let cp = a
    .val("cp")
    .and_then(|v| v.as_vec())
    .map(|p| v3(&p))
    .unwrap_or([0.0; 3]);
  let offset = a.num_or("offset", 0.0);
  let Some(n) = crate::bosl::vecmath::unit_or_none(v) else {
    return a.err("v must be a non-zero direction");
  };

  // Reflection through the plane with normal `n`: subtract twice the
  // component along it.
  let mut m = Mat4::identity();
  for r in 0..3 {
    for c in 0..3 {
      m.0[r * 4 + c] = f64::from(r == c) - 2.0 * n[r] * n[c];
    }
  }
  let mirror = Mat4::translate(cp)
    .mul(&m)
    .mul(&Mat4::translate([-cp[0], -cp[1], -cp[2]]));
  let shift = Mat4::translate([n[0] * offset, n[1] * offset, n[2] * offset]);
  let placements = vec![shift, mirror.mul(&shift)];
  place(lua, a, placements, describe(a, &["v", "cp", "offset"]))
}

// ---------------------------------------------------------------------------
// Distributing a list of shapes by their own sizes
// ---------------------------------------------------------------------------

/// Space a list of shapes out along a direction.
///
/// Unlike the copy functions these take several different shapes and set them
/// apart, so each one's own extent decides how much room it needs.
fn distribute_along(
  lua: &Lua,
  a: &Args,
  dir: V3,
  list_param: &str,
) -> LuaResult<LuaValue> {
  let Some(LuaValue::Table(t)) = a.raw(list_param) else {
    return a.err(format!("{list_param} must be a list of shapes"));
  };
  let mut children: Vec<ScadNode> = Vec::new();
  for i in 1..=t.raw_len() {
    let v: LuaValue = t.get(i)?;
    let LuaValue::UserData(ud) = v else {
      return a.err("every entry must be a shape");
    };
    if let Ok(g) = ud.borrow::<CsgGeometry>() {
      children.push(g.scad.clone().unwrap_or(ScadNode::Union(vec![])));
    } else if let Ok(s) = ud.borrow::<CsgSketch>() {
      children.push(s.scad.clone().unwrap_or(ScadNode::Union(vec![])));
    } else {
      return a.err("every entry must be a shape");
    }
  }
  if children.is_empty() {
    return a.err("the list of shapes cannot be empty");
  }

  // Either a fixed gap between centres, or the shapes' own sizes plus a gap.
  let spacing = a.num("spacing");
  let sizes = a.nums("sizes");
  let axis = if dir[0].abs() > 0.5 {
    0
  } else if dir[1].abs() > 0.5 {
    1
  } else {
    2
  };
  // Each shape needs both how much room it takes and where its middle sits,
  // since a shape built off the origin would otherwise be spaced by its
  // origin rather than by the part you can see.
  let (extents, centres): (Vec<f64>, Vec<f64>) = match sizes {
    Some(s) if s.len() == children.len() => (s, vec![0.0; children.len()]),
    _ => children
      .iter()
      .map(|c| {
        let m = crate::export::materialize_scad_manifold(c);
        let (lo, hi) = m.bounding_box();
        (
          (hi[axis] - lo[axis]) as f64,
          ((hi[axis] + lo[axis]) / 2.0) as f64,
        )
      })
      .unzip(),
  };

  let gap = spacing.unwrap_or(0.0);
  let mut offsets = Vec::with_capacity(children.len());
  let mut cursor = 0.0;
  for (i, e) in extents.iter().enumerate() {
    if i > 0 {
      cursor += extents[i - 1] / 2.0 + gap + e / 2.0;
    }
    offsets.push(cursor);
  }
  // Centre the whole run on the origin.
  let span = offsets.last().copied().unwrap_or(0.0);
  let placed: Vec<ScadNode> = children
    .iter()
    .zip(offsets.iter())
    .zip(centres.iter())
    .map(|((child, o), centre)| {
      let d = o - span / 2.0 - centre;
      wrap_transform(
        child.clone(),
        Mat4::translate([dir[0] * d, dir[1] * d, dir[2] * d]),
      )
    })
    .collect();

  let node = crate::bosl::bosl_node_with_children(
    "std.scad",
    a.func(),
    describe(a, &["spacing", "sizes", "l"]),
    children,
    Some(ScadNode::Union(placed)),
  );
  Ok(LuaValue::UserData(lua.create_userdata(CsgGeometry {
    name: None,
    mesh: None,
    color: None,
    scad: Some(node),
  })?))
}

fn axis_distribute(dir: V3) -> impl Fn(&Lua, &Args) -> LuaResult<LuaValue> {
  move |lua, a| distribute_along(lua, a, dir, "p")
}

fn distribute(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let dir = a
    .val("dir")
    .and_then(|v| v.as_vec())
    .map(|p| v3(&p))
    .unwrap_or([1.0, 0.0, 0.0]);
  distribute_along(lua, a, dir, "p")
}

// ---------------------------------------------------------------------------
// Registration
// ---------------------------------------------------------------------------

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
  add(lua, bosl, "move_copies", &["a", "p"], move_copies)?;
  const COPY_PARAMS: &[&str] = &["spacing", "n", "l", "sp", "p"];
  for (name, dir) in [
    ("xcopies", [1.0, 0.0, 0.0]),
    ("ycopies", [0.0, 1.0, 0.0]),
    ("zcopies", [0.0, 0.0, 1.0]),
  ] {
    add(lua, bosl, name, COPY_PARAMS, axis_copies(dir))?;
  }
  add(
    lua,
    bosl,
    "line_copies",
    &["spacing", "n", "l", "p1", "p2", "p"],
    line_copies,
  )?;
  add(
    lua,
    bosl,
    "grid_copies",
    &["spacing", "n", "size", "stagger", "inside", "nonzero", "p"],
    grid_copies,
  )?;

  add(
    lua,
    bosl,
    "rot_copies",
    &[
      "rots", "v", "cp", "n", "sa", "offset", "delta", "subrot", "p",
    ],
    rot_copies,
  )?;
  const ROT_PARAMS: &[&str] =
    &["rots", "cp", "n", "sa", "r", "d", "subrot", "p"];
  for (name, axis) in [
    ("xrot_copies", [1.0, 0.0, 0.0]),
    ("yrot_copies", [0.0, 1.0, 0.0]),
    ("zrot_copies", [0.0, 0.0, 1.0]),
  ] {
    add(lua, bosl, name, ROT_PARAMS, axis_rot_copies(axis))?;
  }
  add(
    lua,
    bosl,
    "arc_copies",
    &[
      "n", "r", "rx", "ry", "d", "dx", "dy", "sa", "ea", "rot", "cp", "p",
    ],
    arc_copies,
  )?;
  add(
    lua,
    bosl,
    "sphere_copies",
    &["n", "r", "d", "cone_ang", "scale", "perp", "p"],
    sphere_copies,
  )?;
  add(
    lua,
    bosl,
    "path_copies",
    &[
      "path",
      "n",
      "spacing",
      "sp",
      "dist",
      "rotate_children",
      "closed",
      "p",
    ],
    path_copies,
  )?;

  // The names BOSL2 used before it settled on `*_copies`. They take the same
  // arguments and do the same thing, so a script written against the older
  // library still runs.
  add(
    lua,
    bosl,
    "line_of",
    &["spacing", "n", "l", "p1", "p2", "p"],
    line_copies,
  )?;
  add(
    lua,
    bosl,
    "grid2d",
    &["spacing", "n", "size", "stagger", "inside", "nonzero", "p"],
    grid_copies,
  )?;
  add(
    lua,
    bosl,
    "arc_of",
    &[
      "n", "r", "rx", "ry", "d", "dx", "dy", "sa", "ea", "rot", "cp", "p",
    ],
    arc_copies,
  )?;
  add(
    lua,
    bosl,
    "ovoid_spread",
    &["n", "r", "d", "cone_ang", "scale", "perp", "p"],
    sphere_copies,
  )?;
  add(
    lua,
    bosl,
    "path_spread",
    &[
      "path",
      "n",
      "spacing",
      "sp",
      "dist",
      "rotate_children",
      "closed",
      "p",
    ],
    path_copies,
  )?;

  for (name, axis, param) in [
    ("xflip_copy", 0usize, "x"),
    ("yflip_copy", 1, "y"),
    ("zflip_copy", 2, "z"),
  ] {
    let params: &'static [&'static str] = match axis {
      0 => &["offset", "x", "p"],
      1 => &["offset", "y", "p"],
      _ => &["offset", "z", "p"],
    };
    add(lua, bosl, name, params, axis_flip_copy(axis, param))?;
  }
  add(
    lua,
    bosl,
    "mirror_copy",
    &["v", "offset", "cp", "p"],
    mirror_copy,
  )?;

  const DIST_PARAMS: &[&str] = &["spacing", "sizes", "l", "p"];
  for (name, dir) in [
    ("xdistribute", [1.0, 0.0, 0.0]),
    ("ydistribute", [0.0, 1.0, 0.0]),
    ("zdistribute", [0.0, 0.0, 1.0]),
  ] {
    add(lua, bosl, name, DIST_PARAMS, axis_distribute(dir))?;
  }
  add(
    lua,
    bosl,
    "distribute",
    &["spacing", "sizes", "dir", "l", "p"],
    distribute,
  )?;
  Ok(())
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

  fn shape(code: &str) -> crate::scad_export::ScadNode {
    let geoms = crate::lua_engine::execute_lua(code).unwrap();
    geoms[0].scad.clone().unwrap()
  }

  fn volume(code: &str) -> f64 {
    crate::export::materialize_scad_manifold(&shape(code)).volume()
  }

  fn bbox(code: &str) -> ([f32; 3], [f32; 3]) {
    crate::export::materialize_scad_manifold(&shape(code)).bounding_box()
  }

  #[test]
  fn a_copy_run_with_no_target_gives_its_positions() {
    let pts: Vec<Vec<f64>> = eval("return bosl.xcopies({spacing = 10, n = 3})");
    assert_eq!(pts.len(), 3);
    assert_eq!(pts[0][0], -10.0);
    assert_eq!(pts[1][0], 0.0);
    assert_eq!(pts[2][0], 10.0);
  }

  #[test]
  fn a_run_can_be_given_by_length_instead_of_spacing() {
    let pts: Vec<Vec<f64>> = eval("return bosl.xcopies({l = 30, n = 3})");
    assert_eq!(pts[0][0], -15.0);
    assert_eq!(pts[2][0], 15.0);
  }

  #[test]
  fn copies_of_a_shape_add_up_to_the_expected_volume() {
    let v = volume("render(bosl.xcopies({spacing = 20, n = 3, p = cube(5)}))");
    assert!((v - 3.0 * 125.0).abs() < 1e-3, "{v}");
  }

  #[test]
  fn copies_span_the_distance_they_are_spaced_over() {
    let (lo, hi) =
      bbox("render(bosl.xcopies({spacing = 20, n = 3, p = cube(10)}))");
    assert!((lo[0] + 20.0).abs() < 1e-3, "{lo:?}");
    assert!((hi[0] - 30.0).abs() < 1e-3, "{hi:?}");
  }

  #[test]
  fn move_copies_places_one_at_each_point() {
    let v =
      volume("render(bosl.move_copies({{0,0,0},{50,0,0},{0,50,0}}, cube(5)))");
    assert!((v - 3.0 * 125.0).abs() < 1e-3, "{v}");
  }

  #[test]
  fn a_grid_makes_one_copy_per_cell() {
    let v = volume(
      "render(bosl.grid_copies({spacing = 20, n = {3, 2}, p = cube(5)}))",
    );
    assert!((v - 6.0 * 125.0).abs() < 1e-3, "{v}");
  }

  #[test]
  fn rotated_copies_go_all_the_way_round() {
    let pts: Vec<Vec<f64>> = eval("return bosl.zrot_copies({n = 4, r = 10})");
    assert_eq!(pts.len(), 4);
    // Four copies at 90 degree steps, starting on the +X axis.
    assert!((pts[0][0] - 10.0).abs() < 1e-9, "{pts:?}");
    assert!((pts[1][1] - 10.0).abs() < 1e-9, "{pts:?}");
  }

  #[test]
  fn an_arc_of_copies_follows_its_radius() {
    let pts: Vec<Vec<f64>> =
      eval("return bosl.arc_copies({n = 3, r = 10, sa = 0, ea = 180})");
    assert_eq!(pts.len(), 3);
    assert!((pts[0][0] - 10.0).abs() < 1e-9, "{pts:?}");
    assert!((pts[1][1] - 10.0).abs() < 1e-9, "{pts:?}");
    assert!((pts[2][0] + 10.0).abs() < 1e-9, "{pts:?}");
  }

  #[test]
  fn a_flip_copy_doubles_the_shape_across_the_plane() {
    let (lo, hi) = bbox("render(bosl.xflip_copy({offset = 10, p = cube(5)}))");
    assert!((lo[0] + 15.0).abs() < 1e-3, "{lo:?}");
    assert!((hi[0] - 15.0).abs() < 1e-3, "{hi:?}");
    let v = volume("render(bosl.xflip_copy({offset = 10, p = cube(5)}))");
    assert!((v - 250.0).abs() < 1e-3, "{v}");
  }

  #[test]
  fn mirror_copy_reflects_through_an_arbitrary_plane() {
    let v = volume(
      "render(bosl.mirror_copy({v = {1,1,0}, offset = 20, p = cube(5)}))",
    );
    assert!((v - 250.0).abs() < 1e-3, "{v}");
  }

  #[test]
  fn copies_along_a_path_follow_it() {
    let pts: Vec<Vec<f64>> =
      eval("return bosl.path_copies({path = {{0,0,0},{100,0,0}}, n = 3})");
    assert_eq!(pts.len(), 3);
    assert!((pts[0][0]).abs() < 1e-9, "{pts:?}");
    assert!((pts[2][0] - 100.0).abs() < 1e-9, "{pts:?}");
  }

  #[test]
  fn distribute_sets_shapes_apart_by_their_own_size() {
    let (lo, hi) = bbox(
      "render(bosl.xdistribute({spacing = 10, p = {cube(10), cube(20)}}))",
    );
    // 10 wide plus a 10 gap plus 20 wide spans 40 in total.
    assert!((hi[0] - lo[0] - 40.0).abs() < 1e-3, "{lo:?} {hi:?}");
  }

  #[test]
  fn a_copy_run_moves_points_as_readily_as_shapes() {
    let sets: Vec<Vec<Vec<f64>>> =
      eval("return bosl.xcopies({spacing = 10, n = 2, p = {{0,0,0}}})");
    assert_eq!(sets.len(), 2);
    assert_eq!(sets[0][0][0], -5.0);
    assert_eq!(sets[1][0][0], 5.0);
  }

  #[test]
  fn scad_export_writes_the_distributor_call() {
    let scad = crate::scad_export::generate_scad(&[shape(
      "render(bosl.xcopies({spacing = 20, n = 3, p = cube(5)}))",
    )]);
    assert!(scad.contains("xcopies("), "{scad}");
    assert!(scad.contains("cube("), "{scad}");
  }
}
