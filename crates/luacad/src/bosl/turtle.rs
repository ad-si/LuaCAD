//! BOSL2's turtle graphics, from `drawing.scad` and `turtle3d.scad`, plus
//! the last two odds and ends of `drawing.scad` and `polyhedra.scad`.
//!
//! A turtle walks a path by following instructions — move forward, turn
//! left, arc right — rather than by having its points written out. It is the
//! shortest way to describe an outline made of straight runs and tangent
//! arcs, because each arc picks up exactly where the last piece left off and
//! at the same heading.

use mlua::{Lua, Result as LuaResult, Value as LuaValue};

use crate::bosl::value::{Args, PureFn, Val, register_all};
use crate::bosl::vecmath::{self as vm, Mat4, V3};
use crate::scad_export::ScadNode;

/// Where the turtle is, which way it faces, and how it is set up.
#[derive(Debug)]
struct Turtle {
  path: Vec<V3>,
  /// The step vector: its direction is the heading and its length the
  /// distance a bare `move` covers.
  step: V3,
  /// The default turn, in degrees.
  angle: f64,
  /// How many segments an arc is drawn with; zero means pick from the
  /// radius.
  arcsteps: usize,
}

impl Turtle {
  fn here(&self) -> V3 {
    *self.path.last().unwrap_or(&[0.0; 3])
  }

  fn go(&mut self, to: V3) {
    self.path.push(to);
  }

  /// Turn the heading by `deg` about the Z axis.
  fn turn(&mut self, deg: f64) {
    self.step = Mat4::zrot(deg).apply(self.step);
  }

  /// Sweep an arc of `radius` through `sweep` degrees, `side` being +1 to
  /// curve left and -1 to curve right.
  ///
  /// The centre sits one radius off to that side, square to the heading, so
  /// the arc leaves along the current direction and the turtle comes out of
  /// it turned by exactly the swept angle.
  fn arc(&mut self, radius: f64, sweep: f64, side: f64, segments: u32) {
    if sweep == 0.0 || radius == 0.0 {
      return;
    }
    let here = self.here();
    let dir = vm::unit_or(self.step, [1.0, 0.0, 0.0]);
    let normal = [-dir[1], dir[0], 0.0];
    let signed = radius * sweep.signum();
    let centre = vm::add(here, vm::mul(normal, side * signed));
    let steps = if self.arcsteps > 0 {
      self.arcsteps as u32
    } else {
      segments
    };
    let total = sweep.signum() * side * sweep.abs() * radius.signum();
    // The turtle is already standing on the first point of the arc, so only
    // the remaining ones are added — an arc drawn with `n` segments leaves
    // `n` points on the path, not `n + 1`.
    let steps = steps.max(2);
    let rel = vm::sub(here, centre);
    for i in 1..steps {
      let a = total * i as f64 / (steps - 1) as f64;
      self.go(vm::add(centre, Mat4::zrot(a).apply(rel)));
    }
    self.step = Mat4::zrot(side * sweep).apply(self.step);
  }
}

/// Read the command list: names, each optionally followed by one or two
/// numbers.
fn read_commands(a: &Args, name: &str) -> LuaResult<Vec<(String, Vec<f64>)>> {
  let Some(LuaValue::Table(t)) = a.raw(name) else {
    return a.err(format!("{name} must be a list of turtle commands"));
  };
  let mut out: Vec<(String, Vec<f64>)> = Vec::new();
  for i in 1..=t.raw_len() {
    match t.get::<LuaValue>(i) {
      Ok(LuaValue::String(s)) => {
        out.push((s.to_str()?.to_string(), Vec::new()));
      }
      Ok(other) => {
        // A number belongs to the command before it, and a pair of them to
        // the arc commands that take a radius and an angle.
        let Some(nums) = crate::bosl::args::as_nums(&other)
          .or_else(|| crate::bosl::args::as_num(&other).map(|n| vec![n]))
        else {
          return a.err(format!(
            "{name} entry {i} is neither a command nor a number"
          ));
        };
        match out.last_mut() {
          Some(last) => last.1.extend(nums),
          None => return a.err("a turtle list must start with a command"),
        }
      }
      Err(_) => break,
    }
  }
  Ok(out)
}

/// Walk the commands and return the path they trace.
fn walk(
  commands: &[(String, Vec<f64>)],
  start: Turtle,
  repeat: usize,
  segments_for: &dyn Fn(f64) -> u32,
) -> Result<Turtle, String> {
  let mut t = start;
  for _ in 0..repeat.max(1) {
    for (cmd, parms) in commands {
      let p = parms.first().copied();
      let p2 = parms.get(1).copied();
      let here = t.here();
      match cmd.as_str() {
        "move" => {
          let d = p.unwrap_or(1.0);
          t.go(vm::add(here, vm::mul(t.step, d)));
        }
        "xmove" => {
          let d = p.unwrap_or(1.0) * vm::norm(t.step);
          t.go([here[0] + d, here[1], here[2]]);
        }
        "ymove" => {
          let d = p.unwrap_or(1.0) * vm::norm(t.step);
          t.go([here[0], here[1] + d, here[2]]);
        }
        "xymove" => {
          let (dx, dy) = (p.unwrap_or(0.0), p2.unwrap_or(0.0));
          t.go([here[0] + dx, here[1] + dy, here[2]]);
        }
        "jump" => t.go([p.unwrap_or(0.0), p2.unwrap_or(0.0), here[2]]),
        "xjump" => t.go([p.unwrap_or(0.0), here[1], here[2]]),
        "yjump" => t.go([here[0], p.unwrap_or(0.0), here[2]]),
        // Carry on in the current direction until reaching a coordinate.
        "untilx" | "untily" => {
          let axis = usize::from(cmd == "untily");
          let target = p.unwrap_or(0.0);
          let d = t.step[axis];
          if d.abs() < 1e-12 {
            return Err(format!(
              "\"{cmd}\" cannot be reached: the turtle is not heading that way"
            ));
          }
          let k = (target - here[axis]) / d;
          t.go(vm::add(here, vm::mul(t.step, k)));
        }
        "turn" | "left" => t.turn(p.unwrap_or(t.angle)),
        "right" => t.turn(-p.unwrap_or(t.angle)),
        "angle" => t.angle = p.unwrap_or(t.angle),
        "setdir" => {
          let len = vm::norm(t.step);
          match (p, p2) {
            (Some(x), Some(y)) => {
              t.step = vm::mul(vm::unit_or([x, y, 0.0], [1.0, 0.0, 0.0]), len)
            }
            (Some(deg), None) => {
              t.step = [
                len * deg.to_radians().cos(),
                len * deg.to_radians().sin(),
                0.0,
              ]
            }
            _ => return Err("\"setdir\" needs an angle or a direction".into()),
          }
        }
        "length" => {
          let l = p.unwrap_or(1.0);
          t.step = vm::mul(vm::unit_or(t.step, [1.0, 0.0, 0.0]), l);
        }
        "scale" => t.step = vm::mul(t.step, p.unwrap_or(1.0)),
        "addlength" => {
          let extra = p.unwrap_or(0.0);
          let unit = vm::unit_or(t.step, [1.0, 0.0, 0.0]);
          t.step = vm::add(t.step, vm::mul(unit, extra));
        }
        "arcsteps" => t.arcsteps = p.unwrap_or(0.0).max(0.0) as usize,
        "arcleft" | "arcright" => {
          let Some(radius) = p else {
            return Err(format!("\"{cmd}\" needs a radius"));
          };
          let sweep = p2.unwrap_or(t.angle);
          let side = if cmd == "arcleft" { 1.0 } else { -1.0 };
          t.arc(radius, sweep, side, segments_for(radius.abs()));
        }
        "arcleftto" | "arcrightto" => {
          let (Some(radius), Some(target)) = (p, p2) else {
            return Err(format!("\"{cmd}\" needs a radius and a heading"));
          };
          let side = if cmd == "arcleftto" { 1.0 } else { -1.0 };
          // Turn just far enough to end up pointing at the given heading.
          let start = t.step[1].atan2(t.step[0]).to_degrees().rem_euclid(360.0);
          let end = target.rem_euclid(360.0);
          let mut delta = (end - start) * side;
          if delta < 0.0 {
            delta += 360.0;
          }
          t.arc(radius, delta, side, segments_for(radius.abs()));
        }
        other => return Err(format!("unknown turtle command \"{other}\"")),
      }
    }
  }
  Ok(t)
}

fn turtle(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let commands = read_commands(a, "commands")?;
  let repeat = a.int("repeat").unwrap_or(1).max(1) as usize;
  // The state is the path so far, the step vector, the default turn and the
  // arc resolution — the same four things BOSL2 threads through.
  let start = match a.val("state").and_then(|v| v.as_list().map(|s| s.to_vec()))
  {
    Some(items) if items.len() >= 3 => Turtle {
      path: items[0]
        .as_matrix()
        .map(|m| m.iter().map(|p| crate::bosl::value::v3(p)).collect())
        .unwrap_or_else(|| vec![[0.0; 3]]),
      step: items[1]
        .as_vec()
        .map(|v| crate::bosl::value::v3(&v))
        .unwrap_or([1.0, 0.0, 0.0]),
      angle: items[2].as_num().unwrap_or(90.0),
      arcsteps: items.get(3).and_then(|v| v.as_num()).unwrap_or(0.0) as usize,
    },
    // A bare number is a starting step length.
    Some(_) | None => Turtle {
      path: vec![[0.0; 3]],
      step: [a.num("state").unwrap_or(1.0), 0.0, 0.0],
      angle: 90.0,
      arcsteps: 0,
    },
  };

  let segments_for = |r: f64| a.segments(r);
  let out =
    walk(&commands, start, repeat, &segments_for).or_else(|e| a.err(e))?;
  if a.bool_or("full_state", false) {
    return Val::list([
      Val::list(out.path.iter().map(|p| Val::vec([p[0], p[1]]))),
      Val::vec([out.step[0], out.step[1]]),
      Val::Num(out.angle),
      Val::Num(out.arcsteps as f64),
    ])
    .to_lua(lua);
  }
  Val::list(out.path.iter().map(|p| Val::vec([p[0], p[1]]))).to_lua(lua)
}

/// The same walk in three dimensions, returning points or the transforms
/// that would place something at each step.
fn turtle3d(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let commands = read_commands(a, "commands")?;
  let repeat = a.int("repeat").unwrap_or(1).max(1) as usize;
  let step = a
    .val("state")
    .and_then(|v| v.as_vec())
    .map(|v| crate::bosl::value::v3(&v))
    .unwrap_or([1.0, 0.0, 0.0]);

  // Roll, pitch and yaw are the three turns a 3D turtle has; the flat walk
  // handles yaw, and the other two are applied to the heading as they come.
  let mut t = Turtle {
    path: vec![[0.0; 3]],
    step,
    angle: 90.0,
    arcsteps: 0,
  };
  let mut up: V3 = [0.0, 0.0, 1.0];
  for _ in 0..repeat {
    for (cmd, parms) in &commands {
      let p = parms.first().copied();
      match cmd.as_str() {
        "move" => {
          let here = t.here();
          t.go(vm::add(here, vm::mul(t.step, p.unwrap_or(1.0))));
        }
        "up" | "down" | "pitch" => {
          let deg =
            p.unwrap_or(t.angle) * if cmd == "down" { -1.0 } else { 1.0 };
          let axis = vm::cross(t.step, up);
          let m = Mat4::rot_by_axis(axis, deg);
          t.step = m.apply(t.step);
          up = m.apply(up);
        }
        "roll" => {
          let m = Mat4::rot_by_axis(t.step, p.unwrap_or(t.angle));
          up = m.apply(up);
        }
        "left" | "right" | "turn" | "yaw" => {
          let deg =
            p.unwrap_or(t.angle) * if cmd == "right" { -1.0 } else { 1.0 };
          let m = Mat4::rot_by_axis(up, deg);
          t.step = m.apply(t.step);
        }
        "length" => {
          t.step =
            vm::mul(vm::unit_or(t.step, [1.0, 0.0, 0.0]), p.unwrap_or(1.0))
        }
        "scale" => t.step = vm::mul(t.step, p.unwrap_or(1.0)),
        "angle" => t.angle = p.unwrap_or(t.angle),
        "jump" => t.go([
          p.unwrap_or(0.0),
          parms.get(1).copied().unwrap_or(0.0),
          parms.get(2).copied().unwrap_or(0.0),
        ]),
        other => {
          return a.err(format!("unknown 3D turtle command \"{other}\""));
        }
      }
    }
  }
  if a.bool_or("transforms", false) {
    // Each point comes back as the frame that would stand a shape there,
    // facing the way the turtle was going.
    return Val::list(t.path.iter().map(|p| {
      let m =
        Mat4::translate(*p).mul(&Mat4::rot_from_to([0.0, 0.0, 1.0], t.step));
      Val::list((0..4).map(|r| Val::vec((0..4).map(|c| m.0[r * 4 + c]))))
    }))
    .to_lua(lua);
  }
  Val::list(t.path.iter().map(|p| Val::vec(*p))).to_lua(lua)
}

/// The curve a chain hangs in.
///
/// Given a width and how far it droops, or the angle it leaves its supports
/// at, this is the shape that carries its own weight in pure tension — and
/// so, upside down, the arch that carries a load in pure compression.
fn catenary(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let width = a.need_num("width")?;
  let n = a.int("n").unwrap_or(100).max(2) as usize;
  if width <= 0.0 {
    return a.err("width must be positive");
  }
  let droop = a.num("droop");
  let angle = a.num("angle");
  if droop.is_some() == angle.is_some() {
    return a.err("give exactly one of droop and angle");
  }
  let sign = droop.or(angle).map(f64::signum).unwrap_or(1.0);

  // Solve for the scale that gives the asked-for droop or end angle. Both
  // grow with the scale, so a bisection converges from either.
  let target = droop
    .map(|d| d.abs() / (width / 2.0))
    .unwrap_or_else(|| angle.map(f64::abs).unwrap_or(0.0));
  let measure = |x: f64| -> f64 {
    if droop.is_some() {
      (x.cosh() - 1.0) / x
    } else {
      x.sinh().atan().to_degrees()
    }
  };
  if let Some(ang) = angle
    && (ang.abs() >= 90.0 || ang == 0.0)
  {
    return a.err("angle must be between 0 and 90, and not zero");
  }
  let (mut lo, mut hi) = (1e-9f64, 1.0f64);
  while measure(hi) < target && hi < 1e6 {
    hi *= 2.0;
  }
  for _ in 0..200 {
    let mid = (lo + hi) / 2.0;
    if measure(mid) < target {
      lo = mid;
    } else {
      hi = mid;
    }
  }
  let scx = (lo + hi) / 2.0;
  let sc = width / 2.0 / scx;
  let drop = (scx.cosh() - 1.0) * sc;

  let path: Vec<Val> = (0..n)
    .map(|i| {
      let x = -scx + 2.0 * scx * i as f64 / (n - 1) as f64;
      let y = if (x.abs() - scx).abs() < 1e-12 {
        0.0
      } else {
        (x.cosh() - 1.0) * sc - drop
      };
      Val::vec([x * sc, sign * y])
    })
    .collect();
  Val::List(path).to_lua(lua)
}

/// Draw a polygon's vertices and edges, so it can be looked at.
fn debug_polygon(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let Some(points) = a.points2("points") else {
    return a.err("points must be a list of 2D points");
  };
  if points.len() < 3 {
    return a.err("a polygon needs at least three points");
  }
  let size = a.num_or("size", 1.0);
  let mut parts: Vec<ScadNode> = Vec::new();
  for (i, p) in points.iter().enumerate() {
    if a.bool_or("vertices", true) {
      parts.push(ScadNode::Translate {
        x: p[0] as f32,
        y: p[1] as f32,
        z: 0.0,
        child: Box::new(ScadNode::Sphere {
          r: (size / 2.0) as f32,
          segments: 12,
        }),
      });
    }
    if a.bool_or("edges", true) {
      let q = points[(i + 1) % points.len()];
      let d = [q[0] - p[0], q[1] - p[1]];
      let len = (d[0] * d[0] + d[1] * d[1]).sqrt();
      if len > 1e-9 {
        parts.push(ScadNode::Translate {
          x: p[0] as f32,
          y: p[1] as f32,
          z: 0.0,
          child: Box::new(ScadNode::Rotate {
            x: 0.0,
            y: 90.0,
            z: d[1].atan2(d[0]).to_degrees() as f32,
            child: Box::new(ScadNode::Cylinder {
              r1: (size / 3.0) as f32,
              r2: (size / 3.0) as f32,
              h: len as f32,
              center: false,
              segments: 8,
            }),
          }),
        });
      }
    }
  }
  let scad = crate::bosl::bosl_node_with_children(
    "std.scad",
    "debug_polygon",
    a.scad_args().to_string(),
    vec![],
    Some(ScadNode::Union(parts)),
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

/// What a regular polyhedron is made of, by name.
///
/// Reports the vertices, the faces, the three radii — inscribed, midpoint
/// and circumscribed — and the edge length, all scaled to whichever measure
/// the caller pinned down.
fn regular_polyhedron_info(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let name = a
    .string("name")
    .or_else(|| a.string("info"))
    .unwrap_or_else(|| "cube".to_string());
  let phi = (1.0 + 5f64.sqrt()) / 2.0;
  // Each solid is given by its vertices; the faces follow from which of them
  // lie on a common plane at the circumradius.
  let verts: Vec<V3> = match name.as_str() {
    "tetrahedron" => vec![
      [1.0, 1.0, 1.0],
      [1.0, -1.0, -1.0],
      [-1.0, 1.0, -1.0],
      [-1.0, -1.0, 1.0],
    ],
    "cube" | "hexahedron" => (0..8)
      .map(|i| {
        [
          if i & 1 == 0 { -1.0 } else { 1.0 },
          if i & 2 == 0 { -1.0 } else { 1.0 },
          if i & 4 == 0 { -1.0 } else { 1.0 },
        ]
      })
      .collect(),
    "octahedron" => vec![
      [1.0, 0.0, 0.0],
      [-1.0, 0.0, 0.0],
      [0.0, 1.0, 0.0],
      [0.0, -1.0, 0.0],
      [0.0, 0.0, 1.0],
      [0.0, 0.0, -1.0],
    ],
    "dodecahedron" => {
      let mut v: Vec<V3> = (0..8)
        .map(|i| {
          [
            if i & 1 == 0 { -1.0 } else { 1.0 },
            if i & 2 == 0 { -1.0 } else { 1.0 },
            if i & 4 == 0 { -1.0 } else { 1.0 },
          ]
        })
        .collect();
      for (x, y, z) in [
        (0.0, 1.0 / phi, phi),
        (1.0 / phi, phi, 0.0),
        (phi, 0.0, 1.0 / phi),
      ] {
        for sx in [-1.0, 1.0] {
          for sy in [-1.0, 1.0] {
            v.push([x * sx, y * sy, z * if x == 0.0 { sx } else { sy }]);
          }
        }
      }
      v.truncate(20);
      v
    }
    "icosahedron" => {
      let mut v: Vec<V3> = Vec::new();
      for (a0, b0) in [(0usize, 1usize), (1, 2), (2, 0)] {
        for sa in [-1.0, 1.0] {
          for sb in [-1.0, 1.0] {
            let mut p = [0.0; 3];
            p[a0] = sa;
            p[b0] = sb * phi;
            v.push(p);
          }
        }
      }
      v
    }
    other => {
      return a.err(format!(
        "unknown polyhedron '{other}'; the regular ones are tetrahedron, \
         cube, octahedron, dodecahedron and icosahedron"
      ));
    }
  };

  // Scale so that whichever radius or side length was asked for is met.
  let circum = vm::norm(verts[0]);
  let edge = shortest_edge(&verts);
  let wanted = a
    .radius("r", "d", None)
    .or_else(|| a.num("or"))
    .map(|r| r / circum)
    .or_else(|| a.num("side").map(|s| s / edge))
    .unwrap_or(1.0 / circum);
  let verts: Vec<V3> = verts.iter().map(|p| vm::mul(*p, wanted)).collect();

  let Some(faces) = crate::bosl::geom::hull3d(&verts) else {
    return a.err("the vertices do not enclose a volume");
  };
  let t = lua.create_table()?;
  t.set(
    "vertices",
    Val::list(verts.iter().map(|p| Val::vec(*p))).to_lua(lua)?,
  )?;
  t.set(
    "faces",
    Val::list(faces.iter().map(|f| Val::vec(f.iter().map(|i| *i as f64))))
      .to_lua(lua)?,
  )?;
  t.set("name", name)?;
  t.set("or", circum * wanted)?;
  t.set("side", edge * wanted)?;
  Ok(LuaValue::Table(t))
}

fn shortest_edge(verts: &[V3]) -> f64 {
  let mut best = f64::INFINITY;
  for i in 0..verts.len() {
    for j in (i + 1)..verts.len() {
      best = best.min(vm::norm(vm::sub(verts[i], verts[j])));
    }
  }
  best
}

pub fn register(lua: &Lua, bosl: &mlua::Table) -> LuaResult<()> {
  register_all(
    lua,
    bosl,
    &[
      (
        "turtle",
        &["commands", "state", "full_state", "repeat"],
        turtle as PureFn,
      ),
      (
        "turtle3d",
        &["commands", "state", "transforms", "full_state", "repeat"],
        turtle3d,
      ),
      ("catenary", &["width", "droop", "n", "angle"], catenary),
      (
        "debug_polygon",
        &["points", "paths", "vertices", "edges", "convexity", "size"],
        debug_polygon,
      ),
      (
        "regular_polyhedron_info",
        &[
          "info", "name", "index", "type", "faces", "facetype", "hasfaces",
          "side", "ir", "mr", "or", "r", "d", "anchor", "facedown", "stellate",
          "longside", "h", "height",
        ],
        regular_polyhedron_info,
      ),
    ],
  )
}

#[cfg(test)]
mod tests {
  use super::*;

  fn run(cmds: &[(&str, Vec<f64>)]) -> Turtle {
    let commands: Vec<(String, Vec<f64>)> = cmds
      .iter()
      .map(|(c, p)| (c.to_string(), p.clone()))
      .collect();
    walk(
      &commands,
      Turtle {
        path: vec![[0.0; 3]],
        step: [1.0, 0.0, 0.0],
        angle: 90.0,
        arcsteps: 0,
      },
      1,
      &|_| 16,
    )
    .unwrap()
  }

  #[test]
  fn four_moves_and_four_turns_close_a_square() {
    let t = run(&[
      ("move", vec![10.0]),
      ("left", vec![]),
      ("move", vec![10.0]),
      ("left", vec![]),
      ("move", vec![10.0]),
      ("left", vec![]),
      ("move", vec![10.0]),
    ]);
    assert_eq!(t.path.len(), 5);
    let end = t.here();
    assert!(end[0].abs() < 1e-9 && end[1].abs() < 1e-9, "{end:?}");
  }

  #[test]
  fn an_arc_leaves_the_turtle_turned_by_what_it_swept() {
    let t = run(&[("arcleft", vec![10.0, 90.0])]);
    // Started heading +x; a left quarter turn ends heading +y.
    let dir = vm::unit_or(t.step, [0.0; 3]);
    assert!(
      (dir[0]).abs() < 1e-9 && (dir[1] - 1.0).abs() < 1e-9,
      "{dir:?}"
    );
    // A quarter circle of radius 10 from the origin ends at [10, 10].
    let end = t.here();
    assert!(
      (end[0] - 10.0).abs() < 1e-6 && (end[1] - 10.0).abs() < 1e-6,
      "{end:?}"
    );
  }

  #[test]
  fn arcright_curves_the_other_way() {
    let t = run(&[("arcright", vec![10.0, 90.0])]);
    let end = t.here();
    assert!(
      (end[0] - 10.0).abs() < 1e-6 && (end[1] + 10.0).abs() < 1e-6,
      "{end:?}"
    );
  }

  #[test]
  fn untilx_runs_on_to_the_coordinate_asked_for() {
    let t = run(&[("untilx", vec![25.0])]);
    let end = t.here();
    assert!((end[0] - 25.0).abs() < 1e-9, "{end:?}");
  }

  #[test]
  fn jump_moves_without_drawing_a_direction_change() {
    let t = run(&[("jump", vec![5.0, 7.0])]);
    assert_eq!(t.here(), [5.0, 7.0, 0.0]);
    // The heading is untouched.
    assert_eq!(vm::unit_or(t.step, [0.0; 3]), [1.0, 0.0, 0.0]);
  }

  #[test]
  fn an_unknown_command_is_named_in_the_error() {
    let commands = vec![("waggle".to_string(), vec![])];
    let e = walk(
      &commands,
      Turtle {
        path: vec![[0.0; 3]],
        step: [1.0, 0.0, 0.0],
        angle: 90.0,
        arcsteps: 0,
      },
      1,
      &|_| 16,
    )
    .unwrap_err();
    assert!(e.contains("waggle"), "{e}");
  }
}
