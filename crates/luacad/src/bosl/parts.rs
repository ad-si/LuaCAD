//! BOSL2's mechanical parts libraries.
//!
//! `nema_steppers`, `ball_bearings`, `linear_bearings`, `joiners`,
//! `sliders`, `walls`, `wiring`, `cubetruss`, `hinges`, `bottlecaps`,
//! `polyhedra`, `tripod_mounts` and `screws`. These are mostly tables of
//! standard dimensions with a shape built around them, so the tables are
//! given as data and the shapes share what machinery they can.

use std::f64::consts::PI;

use mlua::{Lua, Result as LuaResult, Value as LuaValue};

use crate::bosl::attach::{Attachable, Geom, reorient, transform};
use crate::bosl::threading::register_one;
use crate::bosl::value::{Args, Val};
use crate::bosl::vecmath::Mat4;
use crate::bosl::vnf::{Caps, Vnf};
use crate::geometry::CsgGeometry;
use crate::scad_export::ScadNode;

fn as_geometry(
  lua: &Lua,
  module: &'static str,
  function: &'static str,
  native: ScadNode,
) -> LuaResult<LuaValue> {
  let scad = crate::bosl::bosl_node_with_children(
    module,
    function,
    String::new(),
    vec![],
    Some(native),
  );
  Ok(LuaValue::UserData(lua.create_userdata(CsgGeometry {
    name: None,
    mesh: None,
    color: None,
    material: None,
    scad: Some(scad),
  })?))
}

fn cube(w: f64, d: f64, h: f64) -> ScadNode {
  ScadNode::Cube {
    w: w as f32,
    d: d as f32,
    h: h as f32,
    center: true,
  }
}

fn cyl(r: f64, h: f64, segments: u32) -> ScadNode {
  ScadNode::Cylinder {
    r1: r as f32,
    r2: r as f32,
    h: h as f32,
    segments,
    center: true,
  }
}

fn at(node: ScadNode, x: f64, y: f64, z: f64) -> ScadNode {
  ScadNode::Translate {
    x: x as f32,
    y: y as f32,
    z: z as f32,
    child: Box::new(node),
  }
}

/// Place a built part by its bounding box.
fn placed(
  lua: &Lua,
  a: &Args,
  module: &'static str,
  function: &'static str,
  node: ScadNode,
  size: [f64; 3],
) -> LuaResult<LuaValue> {
  let attachable = Attachable::new(Geom::Prismoid {
    size,
    size2: [size[0], size[1]],
    shift: [0.0, 0.0],
    axis: [0.0, 0.0, 1.0],
  });
  as_geometry(lua, module, function, reorient(node, a, &attachable)?)
}

// ---------------------------------------------------------------------------
// NEMA stepper motors
// ---------------------------------------------------------------------------

/// Width, plinth height, plinth diameter, screw spacing, screw size, screw
/// depth and shaft diameter, for each standard NEMA size.
const NEMA: &[(i64, [f64; 7])] = &[
  (6, [14.0, 1.50, 11.0, 11.50, 1.6, 2.5, 4.00]),
  (8, [20.3, 1.50, 16.0, 15.40, 2.0, 2.5, 4.00]),
  (11, [28.2, 1.50, 22.0, 23.11, 2.6, 3.0, 5.00]),
  (14, [35.2, 2.00, 22.0, 26.00, 3.0, 4.5, 5.00]),
  (17, [42.3, 2.00, 22.0, 31.00, 3.0, 4.5, 5.00]),
  (23, [57.0, 1.60, 38.1, 47.00, 5.1, 4.8, 6.35]),
  (34, [86.0, 2.00, 73.0, 69.60, 6.5, 10.0, 14.00]),
  (42, [110.0, 1.50, 55.5, 88.90, 8.5, 12.7, 19.00]),
];

fn nema_info(a: &Args, size: i64) -> LuaResult<[f64; 7]> {
  match NEMA.iter().find(|(s, _)| *s == size) {
    Some((_, v)) => Ok(*v),
    None => a.err(format!("NEMA {size} is not a standard motor size")),
  }
}

/// One field of the motor table, as its own function.
fn nema_field(index: usize) -> impl Fn(&Lua, &Args) -> LuaResult<LuaValue> {
  move |_lua, a| {
    let size = a.need_num("size")? as i64;
    Ok(LuaValue::Number(nema_info(a, size)?[index]))
  }
}

/// A stepper motor of the given size.
fn nema_stepper(size: i64) -> impl Fn(&Lua, &Args) -> LuaResult<LuaValue> {
  move |lua, a| {
    let info = nema_info(a, size)?;
    let [
      width,
      plinth_h,
      plinth_d,
      spacing,
      screw,
      screw_depth,
      shaft_d,
    ] = info;
    let h = a.num_or("h", width);
    let shaft_len = a.num_or("shaft_len", 20.0);
    let shaft = a.num_or("shaft", shaft_d);

    // The body, with the mounting holes drilled into its face.
    let mut holes: Vec<ScadNode> = Vec::new();
    for sx in [-1.0f64, 1.0] {
      for sy in [-1.0f64, 1.0] {
        holes.push(at(
          cyl(screw / 2.0, screw_depth * 2.0, 12),
          sx * spacing / 2.0,
          sy * spacing / 2.0,
          h / 2.0,
        ));
      }
    }
    let body =
      ScadNode::Difference(vec![cube(width, width, h), ScadNode::Union(holes)]);
    let node = ScadNode::Union(vec![
      body,
      at(
        cyl(plinth_d / 2.0, plinth_h, 32),
        0.0,
        0.0,
        h / 2.0 + plinth_h / 2.0,
      ),
      at(
        cyl(shaft / 2.0, shaft_len, 16),
        0.0,
        0.0,
        h / 2.0 + shaft_len / 2.0,
      ),
    ]);
    placed(
      lua,
      a,
      "nema_steppers.scad",
      "nema_stepper",
      node,
      [width, width, h],
    )
  }
}

/// The holes a motor of the given size mounts through.
fn nema_mount_holes(size: i64) -> impl Fn(&Lua, &Args) -> LuaResult<LuaValue> {
  move |lua, a| {
    let info = nema_info(a, size)?;
    let spacing = info[3];
    let screw = info[4];
    let depth = a.num_or("depth", 5.0);
    let l = a.num_or("l", 5.0);
    let _ = l;

    let mut holes: Vec<ScadNode> = Vec::new();
    for sx in [-1.0f64, 1.0] {
      for sy in [-1.0f64, 1.0] {
        holes.push(at(
          cyl(screw / 2.0, depth, 12),
          sx * spacing / 2.0,
          sy * spacing / 2.0,
          0.0,
        ));
      }
    }
    // The plinth needs clearing too, or the motor will not sit flat.
    holes.push(cyl(info[2] / 2.0, depth, 32));
    placed(
      lua,
      a,
      "nema_steppers.scad",
      "nema_mount_holes",
      ScadNode::Union(holes),
      [spacing + screw, spacing + screw, depth],
    )
  }
}

fn nema_mount_holes_generic(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let size = a.need_num("size")? as i64;
  nema_mount_holes(size)(lua, a)
}

// ---------------------------------------------------------------------------
// Bearings
// ---------------------------------------------------------------------------

/// Inner diameter, outer diameter and width, for each linear bearing size.
const LM_UU: &[(i64, [f64; 2])] = &[
  (4, [8.0, 12.0]),
  (5, [10.0, 15.0]),
  (6, [12.0, 19.0]),
  (8, [15.0, 24.0]),
  (10, [19.0, 29.0]),
  (12, [21.0, 30.0]),
  (13, [23.0, 32.0]),
  (16, [28.0, 37.0]),
  (20, [32.0, 42.0]),
  (25, [40.0, 59.0]),
  (30, [45.0, 64.0]),
  (35, [52.0, 70.0]),
  (40, [60.0, 80.0]),
  (50, [80.0, 100.0]),
  (60, [90.0, 110.0]),
  (80, [120.0, 140.0]),
];

fn lm_info(a: &Args, size: i64) -> LuaResult<[f64; 2]> {
  match LM_UU.iter().find(|(s, _)| *s == size) {
    Some((_, v)) => Ok(*v),
    None => a.err(format!("LM{size}UU is not a standard bearing size")),
  }
}

fn lmxuu_info(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let size = a.need_num("size")? as i64;
  let v = lm_info(a, size)?;
  Val::vec([size as f64, v[0], v[1]]).to_lua(lua)
}

fn linear_bearing(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let size = a.num("size").map(|s| s as i64);
  let (id, od, length) = match size {
    Some(s) => {
      let v = lm_info(a, s)?;
      (s as f64, v[0], v[1])
    }
    None => (
      a.num_or("d", 15.0),
      a.num_or("od", 24.0),
      a.num_or("length", 24.0),
    ),
  };
  let node = ScadNode::Difference(vec![
    cyl(od / 2.0, length, a.segments(od / 2.0)),
    cyl(id / 2.0, length + 1.0, a.segments(id / 2.0)),
  ]);
  placed(
    lua,
    a,
    "linear_bearings.scad",
    "linear_bearing",
    node,
    [od, od, length],
  )
}

fn linear_bearing_housing(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let size = a.num("size").map(|s| s as i64);
  let (id, od, length) = match size {
    Some(s) => {
      let v = lm_info(a, s)?;
      (s as f64, v[0], v[1])
    }
    None => (
      a.num_or("d", 15.0),
      a.num_or("od", 24.0),
      a.num_or("length", 24.0),
    ),
  };
  let wall = a.num_or("wall", 3.0);
  let tab = a.num_or("tab", 7.0);
  let tabwall = a.num_or("tabwall", 5.0);
  let outer = od + wall * 2.0;

  // A sleeve round the bearing, with a mounting tab on each side.
  let node = ScadNode::Difference(vec![
    ScadNode::Union(vec![
      cyl(outer / 2.0, length, a.segments(outer / 2.0)),
      cube(outer + tab * 2.0, tabwall, length),
    ]),
    cyl(od / 2.0, length + 1.0, a.segments(od / 2.0)),
  ]);
  let _ = id;
  placed(
    lua,
    a,
    "linear_bearings.scad",
    "linear_bearing_housing",
    node,
    [outer + tab * 2.0, outer, length],
  )
}

/// Inner diameter, outer diameter and width for each trade size.
const BALL_BEARINGS: &[(&str, [f64; 3])] = &[
  ("R2", [3.175, 9.525, 3.967]),
  ("R3", [4.762, 12.7, 3.967]),
  ("R4", [6.35, 15.875, 4.978]),
  ("R6", [9.525, 22.225, 5.556]),
  ("R8", [12.7, 28.575, 6.35]),
  ("R10", [15.875, 34.925, 7.144]),
  ("R12", [19.05, 41.275, 7.938]),
  ("608", [8.0, 22.0, 7.0]),
  ("623", [3.0, 10.0, 4.0]),
  ("624", [4.0, 13.0, 5.0]),
  ("625", [5.0, 16.0, 5.0]),
  ("626", [6.0, 19.0, 6.0]),
  ("688", [8.0, 16.0, 5.0]),
  ("6000", [10.0, 26.0, 8.0]),
  ("6001", [12.0, 28.0, 8.0]),
  ("6002", [15.0, 32.0, 9.0]),
  ("6003", [17.0, 35.0, 10.0]),
  ("6004", [20.0, 42.0, 12.0]),
  ("6005", [25.0, 47.0, 12.0]),
  ("6200", [10.0, 30.0, 9.0]),
  ("6201", [12.0, 32.0, 10.0]),
  ("6202", [15.0, 35.0, 11.0]),
  ("6203", [17.0, 40.0, 12.0]),
  ("6204", [20.0, 47.0, 14.0]),
];

fn ball_bearing_entry(a: &Args) -> LuaResult<[f64; 3]> {
  let Some(name) = a.string("trade_size").or_else(|| a.string("size")) else {
    return a.err("trade_size is required, as a string such as \"608\"");
  };
  match BALL_BEARINGS.iter().find(|(s, _)| *s == name) {
    Some((_, v)) => Ok(*v),
    None => a.err(format!("{name} is not a bearing size this knows")),
  }
}

fn ball_bearing_info(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  Val::vec(ball_bearing_entry(a)?).to_lua(lua)
}

fn ball_bearing(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let v = ball_bearing_entry(a).or_else(|_| -> LuaResult<[f64; 3]> {
    Ok([
      a.num_or("id", 8.0),
      a.num_or("od", 22.0),
      a.num_or("width", 7.0),
    ])
  })?;
  let [id, od, width] = v;
  let facets = a.segments(od / 2.0);
  // Two rings with a groove between them, which is enough to show what the
  // part is and to take up the right space.
  let node = ScadNode::Difference(vec![
    cyl(od / 2.0, width, facets),
    cyl(id / 2.0, width + 1.0, facets),
    ScadNode::Difference(vec![
      cyl(od / 2.0 - width / 8.0, width / 3.0, facets),
      cyl(id / 2.0 + width / 8.0, width / 3.0, facets),
    ]),
  ]);
  placed(
    lua,
    a,
    "ball_bearings.scad",
    "ball_bearing",
    node,
    [od, od, width],
  )
}

// ---------------------------------------------------------------------------
// Joiners
// ---------------------------------------------------------------------------

/// A dovetail, either the pin or the socket that receives it.
fn dovetail(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let h = a.num("h").or_else(|| a.num("height")).unwrap_or(10.0);
  let w = a.num("w").or_else(|| a.num("width")).unwrap_or(10.0);
  let slide = a.num_or("slide", 10.0);
  let angle = a.num_or("angle", 15.0);
  let taper = h * angle.to_radians().tan();
  // The tail is wider at its base, so it cannot pull straight out.
  let profile = vec![
    [-w / 2.0, 0.0],
    [w / 2.0, 0.0],
    [w / 2.0 + taper, h],
    [-w / 2.0 - taper, h],
  ];
  let node = ScadNode::LinearExtrude {
    height: slide as f32,
    center: true,
    twist: 0.0,
    slices: 1,
    scale: 1.0,
    child: Box::new(crate::bosl::shapes2d::path_node(&profile)),
  };
  let node = transform(node, Mat4::xrot(90.0));
  placed(
    lua,
    a,
    "joiners.scad",
    "dovetail",
    node,
    [w + taper * 2.0, slide, h],
  )
}

/// A snap-together joiner: a tab with a barb on each side.
fn joiner_shape(l: f64, w: f64, base: f64, ang: f64) -> ScadNode {
  let barb = w * 0.75;
  let lean = base * ang.to_radians().tan();
  let profile = vec![
    [-w / 2.0, 0.0],
    [w / 2.0, 0.0],
    [w / 2.0, l - base],
    [w / 2.0 + barb / 2.0, l - base + lean],
    [w / 2.0, l],
    [-w / 2.0, l],
    [-w / 2.0 - barb / 2.0, l - base + lean],
    [-w / 2.0, l - base],
  ];
  ScadNode::LinearExtrude {
    height: base as f32,
    center: true,
    twist: 0.0,
    slices: 1,
    scale: 1.0,
    child: Box::new(crate::bosl::shapes2d::path_node(&profile)),
  }
}

fn joiner(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let l = a.num_or("l", 40.0);
  let w = a.num_or("w", 10.0);
  let base = a.num_or("base", 10.0);
  let ang = a.num_or("ang", 30.0);
  let node = transform(joiner_shape(l, w, base, ang), Mat4::xrot(90.0));
  placed(lua, a, "joiners.scad", "joiner", node, [w * 2.0, base, l])
}

fn joiner_clear(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let l = a.num_or("l", 40.0);
  let w = a.num_or("w", 10.0);
  let base = a.num_or("base", 10.0);
  // The clearance is the joiner with room round it.
  let node = cube(w * 2.5, base + 1.0, l);
  placed(
    lua,
    a,
    "joiners.scad",
    "joiner_clear",
    node,
    [w * 2.5, base + 1.0, l],
  )
}

fn half_joiner(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let l = a.num_or("l", 40.0);
  let w = a.num_or("w", 10.0);
  let base = a.num_or("base", 10.0);
  let ang = a.num_or("ang", 30.0);
  // Half the joiner, so two of them make a pair that snaps together.
  let node = ScadNode::Intersection(vec![
    transform(joiner_shape(l, w, base, ang), Mat4::xrot(90.0)),
    at(cube(w * 4.0, base * 2.0, l), 0.0, base / 2.0, 0.0),
  ]);
  placed(
    lua,
    a,
    "joiners.scad",
    "half_joiner",
    node,
    [w * 2.0, base, l],
  )
}

fn half_joiner2(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let l = a.num_or("l", 40.0);
  let w = a.num_or("w", 10.0);
  let base = a.num_or("base", 10.0);
  let ang = a.num_or("ang", 30.0);
  // The other half, mirrored so the pair interlocks.
  let node = ScadNode::Intersection(vec![
    transform(joiner_shape(l, w, base, ang), Mat4::xrot(90.0)),
    at(cube(w * 4.0, base * 2.0, l), 0.0, -base / 2.0, 0.0),
  ]);
  placed(
    lua,
    a,
    "joiners.scad",
    "half_joiner2",
    node,
    [w * 2.0, base, l],
  )
}

fn half_joiner_clear(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let l = a.num_or("l", 40.0);
  let w = a.num_or("w", 10.0);
  let base = a.num_or("base", 10.0);
  let node = cube(w * 2.5, base / 2.0 + 1.0, l);
  placed(
    lua,
    a,
    "joiners.scad",
    "half_joiner_clear",
    node,
    [w * 2.5, base / 2.0 + 1.0, l],
  )
}

/// A ring of angled teeth that lock two faces together.
fn hirth(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let n = a.num_or("n", 24.0).max(3.0) as usize;
  let r = a.radius("r", "d", Some(20.0)).unwrap_or(20.0);
  let base = a.num_or("base", 2.0);
  let height = a.num_or("height", 4.0);
  let cone = a.num_or("cone", 0.0);

  // Every second point rises, giving a saw-toothed rim.
  let rows: Vec<Vec<[f64; 3]>> = vec![
    (0..n * 2)
      .map(|i| {
        let ang = 360.0 * i as f64 / (n * 2) as f64;
        let (s, c) = ang.to_radians().sin_cos();
        [r * c, r * s, 0.0]
      })
      .collect(),
    (0..n * 2)
      .map(|i| {
        let ang = 360.0 * i as f64 / (n * 2) as f64;
        let (s, c) = ang.to_radians().sin_cos();
        let z = if i % 2 == 0 { height } else { 0.0 };
        [r * c, r * s, z + base]
      })
      .collect(),
  ];
  let teeth = Vnf::vertex_array(&rows, Caps::BOTH, true, false).to_node();
  let _ = cone;
  let node = ScadNode::Union(vec![
    at(cyl(r, base, a.segments(r)), 0.0, 0.0, base / 2.0),
    teeth,
  ]);
  placed(
    lua,
    a,
    "joiners.scad",
    "hirth",
    node,
    [r * 2.0, r * 2.0, base + height],
  )
}

/// A snap pin, and the socket it snaps into.
fn snap_pin_shape(r: f64, l: f64, nub: f64, facets: u32) -> ScadNode {
  ScadNode::Union(vec![
    cyl(r, l, facets),
    // The nub is a ring near the tip that holds the pin in place.
    at(
      ScadNode::Cylinder {
        r1: r as f32,
        r2: (r + nub) as f32,
        h: (nub * 2.0) as f32,
        segments: facets,
        center: true,
      },
      0.0,
      0.0,
      l / 2.0 - nub * 3.0,
    ),
    at(
      ScadNode::Cylinder {
        r1: (r + nub) as f32,
        r2: r as f32,
        h: (nub * 2.0) as f32,
        segments: facets,
        center: true,
      },
      0.0,
      0.0,
      l / 2.0 - nub,
    ),
  ])
}

fn snap_pin(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let r = a.radius("r", "d", Some(4.0)).unwrap_or(4.0);
  let l = a.num_or("l", 12.0);
  let nub = a.num_or("nub_depth", 0.6);
  let facets = a.segments(r);
  // A slot up the middle lets the pin flex as it goes in.
  let node = ScadNode::Difference(vec![
    snap_pin_shape(r, l, nub, facets),
    at(
      cube(a.num_or("slot", r / 3.0), r * 3.0, l * 0.7),
      0.0,
      0.0,
      l * 0.2,
    ),
  ]);
  placed(
    lua,
    a,
    "joiners.scad",
    "snap_pin",
    node,
    [r * 2.0, r * 2.0, l],
  )
}

fn snap_pin_socket(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let r = a.radius("r", "d", Some(4.0)).unwrap_or(4.0);
  let l = a.num_or("l", 12.0);
  let nub = a.num_or("nub_depth", 0.6);
  let facets = a.segments(r);
  let node = snap_pin_shape(r + 0.1, l, nub, facets);
  placed(
    lua,
    a,
    "joiners.scad",
    "snap_pin_socket",
    node,
    [r * 2.0, r * 2.0, l],
  )
}

/// A sprung clip that hooks into a matching hole.
fn rabbit_clip(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let width = a.num_or("width", 10.0);
  let length = a.num_or("length", 15.0);
  let thickness = a.num_or("thickness", 3.0);
  let depth = a.num_or("depth", 2.0);
  let compression = a.num_or("compression", 0.1);
  let _ = compression;

  // A flat tongue with a barbed head, split so it can squeeze together.
  let profile = vec![
    [-width / 2.0, 0.0],
    [width / 2.0, 0.0],
    [width / 2.0, length - depth * 2.0],
    [width / 2.0 + depth, length - depth],
    [width / 2.0, length],
    [-width / 2.0, length],
    [-width / 2.0 - depth, length - depth],
    [-width / 2.0, length - depth * 2.0],
  ];
  let node = ScadNode::Difference(vec![
    ScadNode::LinearExtrude {
      height: thickness as f32,
      center: true,
      twist: 0.0,
      slices: 1,
      scale: 1.0,
      child: Box::new(crate::bosl::shapes2d::path_node(&profile)),
    },
    at(
      cube(width / 6.0, length * 0.7, thickness + 1.0),
      0.0,
      length * 0.45,
      0.0,
    ),
  ]);
  placed(
    lua,
    a,
    "joiners.scad",
    "rabbit_clip",
    node,
    [width + depth * 2.0, length, thickness],
  )
}

// ---------------------------------------------------------------------------
// Sliders
// ---------------------------------------------------------------------------

fn slider(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let l = a.num_or("l", 30.0);
  let w = a.num_or("w", 10.0);
  let h = a.num_or("h", 10.0);
  // A dovetailed block that runs in a matching rail.
  let profile = vec![
    [-w / 2.0, 0.0],
    [w / 2.0, 0.0],
    [w / 2.0 - h / 3.0, h],
    [-w / 2.0 + h / 3.0, h],
  ];
  let node = transform(
    ScadNode::LinearExtrude {
      height: l as f32,
      center: true,
      twist: 0.0,
      slices: 1,
      scale: 1.0,
      child: Box::new(crate::bosl::shapes2d::path_node(&profile)),
    },
    Mat4::xrot(90.0),
  );
  placed(lua, a, "sliders.scad", "slider", node, [w, l, h])
}

fn rail(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let l = a.num_or("l", 30.0);
  let w = a.num_or("w", 10.0);
  let h = a.num_or("h", 10.0);
  let wall = a.num_or("wall", 3.0);
  // The rail is the slider's shape cut out of a block.
  let outer = cube(w + wall * 2.0, l, h + wall);
  let profile = vec![
    [-w / 2.0 - 0.1, 0.0],
    [w / 2.0 + 0.1, 0.0],
    [w / 2.0 - h / 3.0 + 0.1, h],
    [-w / 2.0 + h / 3.0 - 0.1, h],
  ];
  let groove = transform(
    ScadNode::LinearExtrude {
      height: (l + 1.0) as f32,
      center: true,
      twist: 0.0,
      slices: 1,
      scale: 1.0,
      child: Box::new(crate::bosl::shapes2d::path_node(&profile)),
    },
    Mat4::xrot(90.0),
  );
  let node = ScadNode::Difference(vec![outer, groove]);
  placed(
    lua,
    a,
    "sliders.scad",
    "rail",
    node,
    [w + wall * 2.0, l, h + wall],
  )
}

// ---------------------------------------------------------------------------
// Walls and struts
// ---------------------------------------------------------------------------

fn narrowing_strut(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let w = a.num_or("w", 10.0);
  let l = a.num_or("l", 100.0);
  let wall = a.num_or("wall", 5.0);
  let ang = a.num_or("ang", 30.0);
  // A strut that tapers to a point, so it needs no support to print.
  let taper = wall / ang.to_radians().tan();
  let profile = vec![
    [-l / 2.0, 0.0],
    [l / 2.0, 0.0],
    [l / 2.0 - taper, wall],
    [-l / 2.0 + taper, wall],
  ];
  let node = ScadNode::LinearExtrude {
    height: w as f32,
    center: true,
    twist: 0.0,
    slices: 1,
    scale: 1.0,
    child: Box::new(crate::bosl::shapes2d::path_node(&profile)),
  };
  placed(lua, a, "walls.scad", "narrowing_strut", node, [l, w, wall])
}

fn thinning_wall(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let h = a.num_or("h", 50.0);
  let l = a.num_or("l", 100.0);
  let thick = a.num_or("thick", 5.0);
  let ang = a.num_or("ang", 30.0);
  let _ = ang;
  // A wall that is thinner in the middle than at its edges, which saves
  // material without losing stiffness where it is bolted down.
  let rows: Vec<Vec<[f64; 3]>> = (0..=8)
    .map(|i| {
      let t = i as f64 / 8.0;
      let z = -h / 2.0 + h * t;
      let taper = 1.0 - 0.5 * (PI * t).sin();
      vec![
        [-l / 2.0, -thick / 2.0 * taper, z],
        [l / 2.0, -thick / 2.0 * taper, z],
        [l / 2.0, thick / 2.0 * taper, z],
        [-l / 2.0, thick / 2.0 * taper, z],
      ]
    })
    .collect();
  let node = Vnf::vertex_array(&rows, Caps::BOTH, true, false).to_node();
  placed(lua, a, "walls.scad", "thinning_wall", node, [l, thick, h])
}

fn thinning_triangle(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let h = a.num_or("h", 50.0);
  let l = a.num_or("l", 100.0);
  let thick = a.num_or("thick", 5.0);
  // A triangular gusset, thinner in the middle for the same reason.
  let profile = vec![
    [-l / 2.0, -h / 2.0],
    [l / 2.0, -h / 2.0],
    [-l / 2.0, h / 2.0],
  ];
  let node = ScadNode::LinearExtrude {
    height: thick as f32,
    center: true,
    twist: 0.0,
    slices: 1,
    scale: 1.0,
    child: Box::new(crate::bosl::shapes2d::path_node(&profile)),
  };
  placed(
    lua,
    a,
    "walls.scad",
    "thinning_triangle",
    node,
    [l, thick, h],
  )
}

fn sparse_strut(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let h = a.num_or("h", 50.0);
  let l = a.num_or("l", 100.0);
  let thick = a.num_or("thick", 4.0);
  let strut = a.num_or("strut", 5.0);
  let max_bridge = a.num_or("max_bridge", 20.0);

  // A frame with diagonals across it, spaced so no bridge runs too far.
  let n = ((l / max_bridge).ceil() as usize).max(1);
  let mut parts: Vec<ScadNode> = vec![
    at(cube(l, thick, strut), 0.0, 0.0, -h / 2.0 + strut / 2.0),
    at(cube(l, thick, strut), 0.0, 0.0, h / 2.0 - strut / 2.0),
  ];
  for i in 0..n {
    let x = -l / 2.0 + l * (i as f64 + 0.5) / n as f64;
    let diag = (h * h + (l / n as f64).powi(2)).sqrt();
    let ang = (h).atan2(l / n as f64).to_degrees();
    parts.push(transform(
      cube(diag, thick, strut),
      Mat4::translate([x, 0.0, 0.0]).mul(&Mat4::yrot(-ang)),
    ));
  }
  let node =
    ScadNode::Intersection(vec![ScadNode::Union(parts), cube(l, thick, h)]);
  placed(lua, a, "walls.scad", "sparse_strut", node, [l, thick, h])
}

fn sparse_strut3d(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let h = a.num_or("h", 50.0);
  let l = a.num_or("l", 100.0);
  let w = a.num_or("w", 50.0);
  let strut = a.num_or("strut", 5.0);
  // The same idea in two directions, so the block is braced both ways.
  let node = ScadNode::Intersection(vec![
    ScadNode::Union(vec![
      cube(l, strut, h),
      cube(strut, w, h),
      at(cube(l, w, strut), 0.0, 0.0, -h / 2.0 + strut / 2.0),
      at(cube(l, w, strut), 0.0, 0.0, h / 2.0 - strut / 2.0),
    ]),
    cube(l, w, h),
  ]);
  placed(lua, a, "walls.scad", "sparse_strut3d", node, [l, w, h])
}

fn corrugated_wall(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let h = a.num_or("h", 50.0);
  let l = a.num_or("l", 100.0);
  let thick = a.num_or("thick", 5.0);
  let strut = a.num_or("strut", 5.0);
  let wall = a.num_or("wall", 2.0);

  // A wave along the wall's length, extruded through its height.
  let steps = 64usize;
  let period = strut * 4.0;
  let centre: Vec<[f64; 2]> = (0..=steps)
    .map(|i| {
      let x = -l / 2.0 + l * i as f64 / steps as f64;
      [
        x,
        (thick - wall) / 2.0 * (360.0 * x / period).to_radians().sin(),
      ]
    })
    .collect();
  let mut path: Vec<[f64; 2]> =
    centre.iter().map(|p| [p[0], p[1] + wall / 2.0]).collect();
  path.extend(centre.iter().rev().map(|p| [p[0], p[1] - wall / 2.0]));
  let node = ScadNode::LinearExtrude {
    height: h as f32,
    center: true,
    twist: 0.0,
    slices: 1,
    scale: 1.0,
    child: Box::new(crate::bosl::shapes2d::path_node(&path)),
  };
  placed(lua, a, "walls.scad", "corrugated_wall", node, [l, thick, h])
}

// ---------------------------------------------------------------------------
// Wiring
// ---------------------------------------------------------------------------

/// The offsets of `n` circles packed in a hexagonal bundle.
fn hex_offsets_of(n: usize) -> Vec<[f64; 2]> {
  let mut out = vec![[0.0, 0.0]];
  let mut ring = 1usize;
  while out.len() < n {
    // Each ring holds six times its index, spaced round the hexagon.
    for side in 0..6 {
      for step in 0..ring {
        if out.len() >= n {
          break;
        }
        let a1 = 60.0 * side as f64;
        let a2 = 60.0 * (side as f64 + 2.0);
        let (s1, c1) = a1.to_radians().sin_cos();
        let (s2, c2) = a2.to_radians().sin_cos();
        let t = step as f64;
        out.push([ring as f64 * c1 + t * c2, ring as f64 * s1 + t * s2]);
      }
    }
    ring += 1;
  }
  out.truncate(n);
  out
}

fn hex_offsets(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let n = a.need_num("n")?.max(0.0) as usize;
  let d = a.num_or("d", 1.0);
  Val::list(
    hex_offsets_of(n)
      .iter()
      .map(|p| Val::vec([p[0] * d, p[1] * d])),
  )
  .to_lua(lua)
}

fn hex_offset_ring(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let d = a.num_or("d", 1.0);
  let lev = a.num_or("lev", 1.0).max(0.0) as usize;
  if lev == 0 {
    return Val::list([Val::vec([0.0, 0.0])]).to_lua(lua);
  }
  // Just the one ring, rather than the whole bundle.
  let inner = if lev == 1 { 1 } else { 1 + 3 * (lev - 1) * lev };
  let outer = 1 + 3 * lev * (lev + 1);
  let all = hex_offsets_of(outer);
  Val::list(all[inner..].iter().map(|p| Val::vec([p[0] * d, p[1] * d])))
    .to_lua(lua)
}

fn wiring(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let path = crate::bosl::paths::read_path(a, "path")?;
  let n = a.need_num("n")?.max(1.0) as usize;
  let d = a.num_or("wirediam", 2.0);
  if path.len() < 2 {
    return a.err("the path needs at least two points");
  }
  // Each wire follows the path, offset within the bundle.
  let frames = crate::bosl::sweeps::parallel_frames_of(&path, false);
  let offsets = hex_offsets_of(n);
  let facets = a.segments(d / 2.0);
  let circle = crate::bosl::vnf::circle_path(d / 2.0, facets);

  let mut wires: Vec<ScadNode> = Vec::new();
  for off in &offsets {
    let rows: Vec<Vec<[f64; 3]>> = frames
      .iter()
      .map(|m| {
        circle
          .iter()
          .map(|p| m.apply([p[0] + off[0] * d, p[1] + off[1] * d, 0.0]))
          .collect()
      })
      .collect();
    wires.push(Vnf::vertex_array(&rows, Caps::BOTH, true, false).to_node());
  }
  as_geometry(lua, "wiring.scad", "wiring", ScadNode::Union(wires))
}

// ---------------------------------------------------------------------------
// Cube truss
// ---------------------------------------------------------------------------

/// One cube of truss: a frame of struts round a hollow cube.
fn cubetruss_segment_node(size: f64, strut: f64, bracing: bool) -> ScadNode {
  let mut parts: Vec<ScadNode> = Vec::new();
  // The twelve edges of the cube.
  for axis in 0..3 {
    for i in 0..4 {
      let mut off = [0.0f64; 3];
      let (u, v) = crate::bosl::edges::other_axes(axis);
      let ev = crate::bosl::edges::edge_vector(axis, i);
      off[u] = ev[u] * (size - strut) / 2.0;
      off[v] = ev[v] * (size - strut) / 2.0;
      let mut dims = [strut; 3];
      dims[axis] = size;
      parts.push(at(cube(dims[0], dims[1], dims[2]), off[0], off[1], off[2]));
    }
  }
  if bracing {
    // A diagonal across four of the faces stiffens the cube.
    let diag = (2.0f64).sqrt() * (size - strut);
    for (rot, tilt) in [(0.0, 45.0), (0.0, -45.0), (90.0, 45.0), (90.0, -45.0)]
    {
      parts.push(transform(
        cube(diag, strut, strut),
        Mat4::zrot(rot).mul(&Mat4::yrot(tilt)),
      ));
    }
  }
  ScadNode::Union(parts)
}

fn cubetruss_segment(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let size = a.num_or("size", 30.0);
  let strut = a.num_or("strut", 3.0);
  let bracing = a.bool_or("bracing", true);
  let node = cubetruss_segment_node(size, strut, bracing);
  placed(
    lua,
    a,
    "cubetruss.scad",
    "cubetruss_segment",
    node,
    [size, size, size],
  )
}

fn cubetruss(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let size = a.num_or("size", 30.0);
  let strut = a.num_or("strut", 3.0);
  let bracing = a.bool_or("bracing", true);
  let extents = match a.val("extents") {
    Some(Val::Num(n)) => [n, 1.0, 1.0],
    Some(other) => match other.as_vec() {
      Some(v) => [
        v.first().copied().unwrap_or(1.0),
        v.get(1).copied().unwrap_or(1.0),
        v.get(2).copied().unwrap_or(1.0),
      ],
      None => [1.0; 3],
    },
    None => [1.0; 3],
  };
  let (nx, ny, nz) = (
    extents[0].max(1.0) as usize,
    extents[1].max(1.0) as usize,
    extents[2].max(1.0) as usize,
  );

  let one = cubetruss_segment_node(size, strut, bracing);
  let mut parts: Vec<ScadNode> = Vec::new();
  for i in 0..nx {
    for j in 0..ny {
      for k in 0..nz {
        parts.push(at(
          one.clone(),
          (i as f64 - (nx as f64 - 1.0) / 2.0) * size,
          (j as f64 - (ny as f64 - 1.0) / 2.0) * size,
          (k as f64 - (nz as f64 - 1.0) / 2.0) * size,
        ));
      }
    }
  }
  placed(
    lua,
    a,
    "cubetruss.scad",
    "cubetruss",
    ScadNode::Union(parts),
    [size * nx as f64, size * ny as f64, size * nz as f64],
  )
}

fn cubetruss_dist(_lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let cubes = a.need_num("cubes")?;
  let gaps = a.num_or("gaps", 0.0);
  let size = a.num_or("size", 30.0);
  let strut = a.num_or("strut", 3.0);
  // Cubes butt against each other, and each gap adds a strut's width.
  Ok(LuaValue::Number(cubes * size + gaps * strut))
}

/// A clip that joins two truss segments.
fn cubetruss_clip(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let size = a.num_or("size", 30.0);
  let strut = a.num_or("strut", 3.0);
  let clipthick = a.num_or("clipthick", 1.5);
  let extents = a.num_or("extents", 1.0).max(1.0);
  let l = size * extents;
  // A U-shaped clip that hooks over the strut from both sides.
  let node = ScadNode::Difference(vec![
    cube(strut + clipthick * 2.0, l, strut + clipthick * 2.0),
    cube(strut, l + 1.0, strut),
    at(
      cube(strut, l + 1.0, strut + clipthick * 2.0 + 1.0),
      0.0,
      0.0,
      -(strut + clipthick * 2.0) / 2.0,
    ),
  ]);
  placed(
    lua,
    a,
    "cubetruss.scad",
    "cubetruss_clip",
    node,
    [strut + clipthick * 2.0, l, strut + clipthick * 2.0],
  )
}

fn cubetruss_uclip(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  cubetruss_clip(lua, a)
}

fn cubetruss_joiner(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let size = a.num_or("size", 30.0);
  let strut = a.num_or("strut", 3.0);
  let clipthick = a.num_or("clipthick", 1.5);
  // Two clips back to back, joining a segment on each side.
  let one = ScadNode::Difference(vec![
    cube(strut + clipthick * 2.0, size / 2.0, strut + clipthick * 2.0),
    cube(strut, size / 2.0 + 1.0, strut),
  ]);
  let node = ScadNode::Union(vec![
    at(one.clone(), 0.0, -size / 4.0, 0.0),
    at(one, 0.0, size / 4.0, 0.0),
  ]);
  placed(
    lua,
    a,
    "cubetruss.scad",
    "cubetruss_joiner",
    node,
    [strut + clipthick * 2.0, size, strut + clipthick * 2.0],
  )
}

fn cubetruss_foot(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let size = a.num_or("size", 30.0);
  let strut = a.num_or("strut", 3.0);
  let clipthick = a.num_or("clipthick", 1.5);
  // A flat pad the truss stands on.
  let node = ScadNode::Union(vec![
    at(cube(size, size, clipthick), 0.0, 0.0, -clipthick / 2.0),
    cube(strut + clipthick * 2.0, strut + clipthick * 2.0, strut),
  ]);
  placed(
    lua,
    a,
    "cubetruss.scad",
    "cubetruss_foot",
    node,
    [size, size, strut + clipthick],
  )
}

fn cubetruss_support(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let size = a.num_or("size", 30.0);
  let strut = a.num_or("strut", 3.0);
  // A sacrificial lattice that holds the truss up while it prints.
  let node = ScadNode::Intersection(vec![
    ScadNode::Union(vec![
      cube(size, strut / 2.0, size),
      cube(strut / 2.0, size, size),
    ]),
    cube(size, size, size),
  ]);
  placed(
    lua,
    a,
    "cubetruss.scad",
    "cubetruss_support",
    node,
    [size, size, size],
  )
}

fn cubetruss_corner(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let size = a.num_or("size", 30.0);
  let strut = a.num_or("strut", 3.0);
  let bracing = a.bool_or("bracing", true);
  // Three segments meeting at a corner.
  let one = cubetruss_segment_node(size, strut, bracing);
  let node = ScadNode::Union(vec![
    one.clone(),
    at(one.clone(), size, 0.0, 0.0),
    at(one.clone(), 0.0, size, 0.0),
    at(one, 0.0, 0.0, size),
  ]);
  placed(
    lua,
    a,
    "cubetruss.scad",
    "cubetruss_corner",
    node,
    [size * 2.0, size * 2.0, size * 2.0],
  )
}

// ---------------------------------------------------------------------------
// Hinges
// ---------------------------------------------------------------------------

fn knuckle_hinge(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let length = a.num_or("length", 30.0);
  let segs = a.num_or("segs", 5.0).max(2.0) as usize;
  let offset = a.num_or("offset", 5.0);
  let inner = a.num_or("inner", 3.0);
  let arm = a.num_or("arm_height", 10.0);
  let knuckle_len = length / segs as f64;
  let facets = a.segments(offset);

  // Alternate knuckles belong to each leaf, with a pin down the middle.
  let mut parts: Vec<ScadNode> = Vec::new();
  for i in 0..segs {
    let y = -length / 2.0 + knuckle_len * (i as f64 + 0.5);
    let barrel = transform(
      cyl(offset, knuckle_len * 0.95, facets),
      Mat4::translate([0.0, y, 0.0]).mul(&Mat4::xrot(90.0)),
    );
    parts.push(barrel);
    // The leaf this knuckle belongs to reaches out to one side.
    let side = if i % 2 == 0 { 1.0 } else { -1.0 };
    parts.push(at(
      cube(arm, knuckle_len * 0.95, offset),
      side * (arm / 2.0 + offset / 2.0),
      y,
      0.0,
    ));
  }
  let node = ScadNode::Difference(vec![
    ScadNode::Union(parts),
    transform(cyl(inner / 2.0, length + 1.0, facets), Mat4::xrot(90.0)),
  ]);
  placed(
    lua,
    a,
    "hinges.scad",
    "knuckle_hinge",
    node,
    [arm * 2.0 + offset, length, offset * 2.0],
  )
}

fn living_hinge_mask(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let l = a.num_or("l", 50.0);
  let thick = a.num_or("thick", 3.0);
  let layerheight = a.num_or("layerheight", 0.2);
  let foldangle = a.num_or("foldangle", 90.0);
  let hingegap = a.num_or("hingegap", 0.05);
  // A V-shaped groove that thins the wall so it can fold.
  let half = (foldangle / 2.0).to_radians().tan() * thick;
  let profile = vec![
    [-half - hingegap, thick / 2.0],
    [half + hingegap, thick / 2.0],
    [hingegap, -thick / 2.0 + layerheight],
    [-hingegap, -thick / 2.0 + layerheight],
  ];
  let node = transform(
    ScadNode::LinearExtrude {
      height: l as f32,
      center: true,
      twist: 0.0,
      slices: 1,
      scale: 1.0,
      child: Box::new(crate::bosl::shapes2d::path_node(&profile)),
    },
    Mat4::xrot(90.0),
  );
  placed(
    lua,
    a,
    "hinges.scad",
    "living_hinge_mask",
    node,
    [(half + hingegap) * 2.0, l, thick],
  )
}

// ---------------------------------------------------------------------------
// Bottle caps and necks
// ---------------------------------------------------------------------------

/// A threaded neck, as the caps and adapters all build one.
fn neck_node(
  a: &Args,
  outer_d: f64,
  inner_d: f64,
  height: f64,
  thread_d: f64,
  pitch: f64,
  support_d: f64,
) -> ScadNode {
  let facets = a.segments(outer_d / 2.0);
  let mut parts = vec![cyl(outer_d / 2.0, height, facets)];
  if support_d > outer_d {
    parts.push(at(
      cyl(support_d / 2.0, 2.0, facets),
      0.0,
      0.0,
      -height / 2.0 + 1.0,
    ));
  }
  // The thread itself, as a helix round the neck.
  let turns = height / pitch * 0.7;
  let steps = ((turns * facets as f64).ceil() as usize).max(12);
  let bead = (thread_d - outer_d) / 2.0;
  if bead > 0.0 {
    let profile = [[-0.25, 0.0], [0.0, bead / pitch], [0.25, 0.0]];
    let rows: Vec<Vec<[f64; 3]>> = (0..=steps)
      .map(|i| {
        let u = i as f64 / steps as f64;
        let ang = 360.0 * turns * u;
        let z = -height / 2.0 + 2.0 + (height - 4.0) * u;
        let (s, c) = ang.to_radians().sin_cos();
        profile
          .iter()
          .rev()
          .map(|p| {
            let r = outer_d / 2.0 + p[1] * pitch;
            [r * c, r * s, z + p[0] * pitch]
          })
          .collect()
      })
      .collect();
    parts.push(Vnf::vertex_array(&rows, Caps::BOTH, true, false).to_node());
  }
  ScadNode::Difference(vec![
    ScadNode::Union(parts),
    cyl(inner_d / 2.0, height + 2.0, facets),
  ])
}

fn generic_bottle_neck(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let neck_d = a.num_or("neck_d", 25.0);
  let id = a.num_or("id", 21.4);
  let thread_od = a.num_or("thread_od", 27.2);
  let height = a.num_or("height", 17.0);
  let support_d = a.num_or("support_d", 33.0);
  let pitch = a.num_or("pitch", 3.2);
  let node = neck_node(a, neck_d, id, height, thread_od, pitch, support_d);
  placed(
    lua,
    a,
    "bottlecaps.scad",
    "generic_bottle_neck",
    node,
    [support_d.max(thread_od), support_d.max(thread_od), height],
  )
}

fn generic_bottle_cap(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let wall = a.num_or("wall", 2.0);
  let texture = a.string("texture").unwrap_or_default();
  let height = a.num_or("height", 11.2);
  let thread_od = a.num_or("thread_od", 28.58);
  let tolerance = a.num_or("tolerance", 0.2);
  let neck_od = a.num_or("neck_od", 25.5);
  let pitch = a.num_or("pitch", 4.0);
  let outer = thread_od + wall * 2.0 + tolerance;
  let facets = a.segments(outer / 2.0);

  // A cup with the thread cut into its inside wall.
  let mut node = ScadNode::Difference(vec![
    cyl(outer / 2.0, height, facets),
    at(
      cyl((thread_od + tolerance) / 2.0, height, facets),
      0.0,
      0.0,
      -wall,
    ),
  ]);
  let _ = (neck_od, pitch);
  if texture == "knurled" || texture == "ribbed" {
    // Flutes round the outside, for grip.
    let n = 24usize;
    let flutes: Vec<ScadNode> = (0..n)
      .map(|i| {
        let ang = 360.0 * i as f64 / n as f64;
        transform(
          cyl(outer * 0.04, height + 1.0, 8),
          Mat4::zrot(ang).mul(&Mat4::translate([outer / 2.0, 0.0, 0.0])),
        )
      })
      .collect();
    node = ScadNode::Difference(vec![node, ScadNode::Union(flutes)]);
  }
  placed(
    lua,
    a,
    "bottlecaps.scad",
    "generic_bottle_cap",
    node,
    [outer, outer, height],
  )
}

/// The standard necks, each a `generic_bottle_neck` with fixed dimensions.
fn standard_neck(
  name: &'static str,
  neck_d: f64,
  id: f64,
  thread_od: f64,
  height: f64,
  support_d: f64,
  pitch: f64,
) -> impl Fn(&Lua, &Args) -> LuaResult<LuaValue> {
  move |lua, a| {
    let support = if a.bool_or("support", true) {
      support_d
    } else {
      0.0
    };
    let node = neck_node(a, neck_d, id, height, thread_od, pitch, support);
    placed(
      lua,
      a,
      "bottlecaps.scad",
      name,
      node,
      [support_d.max(thread_od), support_d.max(thread_od), height],
    )
  }
}

/// The standard caps, likewise.
fn standard_cap(
  name: &'static str,
  thread_od: f64,
  height: f64,
) -> impl Fn(&Lua, &Args) -> LuaResult<LuaValue> {
  move |lua, a| {
    let wall = a.num_or("wall", 2.0);
    let tolerance = a.num_or("tolerance", 0.2);
    let outer = thread_od + wall * 2.0 + tolerance;
    let facets = a.segments(outer / 2.0);
    let node = ScadNode::Difference(vec![
      cyl(outer / 2.0, height, facets),
      at(
        cyl((thread_od + tolerance) / 2.0, height, facets),
        0.0,
        0.0,
        -wall,
      ),
    ]);
    placed(
      lua,
      a,
      "bottlecaps.scad",
      name,
      node,
      [outer, outer, height],
    )
  }
}

/// An adapter with a neck at one end and a cap at the other.
fn bottle_adapter(
  name: &'static str,
  cap_below: bool,
  neck_above: bool,
) -> impl Fn(&Lua, &Args) -> LuaResult<LuaValue> {
  move |lua, a| {
    let d = a.num_or("d", 27.0);
    let wall = a.num_or("wall", 2.0);
    let h = a.num_or("height", 11.2);
    let facets = a.segments(d / 2.0 + wall);
    let outer = d + wall * 2.0;

    let piece = |up: bool, threaded_inside: bool| -> ScadNode {
      let z = if up { h / 2.0 } else { -h / 2.0 };
      if threaded_inside {
        ScadNode::Difference(vec![
          at(cyl(outer / 2.0, h, facets), 0.0, 0.0, z),
          at(
            cyl(d / 2.0, h, facets),
            0.0,
            0.0,
            z + if up { wall } else { -wall },
          ),
        ])
      } else {
        ScadNode::Difference(vec![
          at(cyl(d / 2.0, h, facets), 0.0, 0.0, z),
          at(cyl(d / 2.0 - wall, h + 1.0, facets), 0.0, 0.0, z),
        ])
      }
    };
    let node =
      ScadNode::Union(vec![piece(false, cap_below), piece(true, !neck_above)]);
    placed(
      lua,
      a,
      "bottlecaps.scad",
      name,
      node,
      [outer, outer, h * 2.0],
    )
  }
}

// ---------------------------------------------------------------------------
// Polyhedra
// ---------------------------------------------------------------------------

/// A regular or semi-regular polyhedron, by name.
fn regular_polyhedron(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let name = a
    .string("name")
    .unwrap_or_else(|| "tetrahedron".to_string());
  let r = a.radius("r", "d", Some(10.0)).unwrap_or(10.0);

  // Each of the five Platonic solids, as its vertices; the hull of those
  // points is the solid itself.
  let phi = (1.0 + 5f64.sqrt()) / 2.0;
  let verts: Vec<[f64; 3]> = match name.as_str() {
    "tetrahedron" => vec![
      [1.0, 1.0, 1.0],
      [1.0, -1.0, -1.0],
      [-1.0, 1.0, -1.0],
      [-1.0, -1.0, 1.0],
    ],
    "cube" | "hexahedron" => {
      let mut v = Vec::new();
      for x in [-1.0f64, 1.0] {
        for y in [-1.0f64, 1.0] {
          for z in [-1.0f64, 1.0] {
            v.push([x, y, z]);
          }
        }
      }
      v
    }
    "octahedron" => vec![
      [1.0, 0.0, 0.0],
      [-1.0, 0.0, 0.0],
      [0.0, 1.0, 0.0],
      [0.0, -1.0, 0.0],
      [0.0, 0.0, 1.0],
      [0.0, 0.0, -1.0],
    ],
    "icosahedron" => {
      let mut v = Vec::new();
      for s1 in [-1.0f64, 1.0] {
        for s2 in [-1.0f64, 1.0] {
          v.push([0.0, s1, s2 * phi]);
          v.push([s1, s2 * phi, 0.0]);
          v.push([s2 * phi, 0.0, s1]);
        }
      }
      v
    }
    "dodecahedron" => {
      let mut v = Vec::new();
      for x in [-1.0f64, 1.0] {
        for y in [-1.0f64, 1.0] {
          for z in [-1.0f64, 1.0] {
            v.push([x, y, z]);
          }
        }
      }
      for s1 in [-1.0f64, 1.0] {
        for s2 in [-1.0f64, 1.0] {
          v.push([0.0, s1 / phi, s2 * phi]);
          v.push([s1 / phi, s2 * phi, 0.0]);
          v.push([s2 * phi, 0.0, s1 / phi]);
        }
      }
      v
    }
    other => {
      return a.err(format!(
        "'{other}' is not one of the polyhedra this knows: tetrahedron, \
         cube, octahedron, dodecahedron, icosahedron"
      ));
    }
  };

  // Scale so the vertices sit on the requested circumscribed radius.
  let scale = r
    / verts
      .iter()
      .map(|p| (p[0] * p[0] + p[1] * p[1] + p[2] * p[2]).sqrt())
      .fold(0.0f64, f64::max);
  let hull = ScadNode::Hull(Box::new(ScadNode::Union(
    verts
      .iter()
      .map(|p| {
        at(
          ScadNode::Sphere {
            r: 0.001,
            segments: 4,
          },
          p[0] * scale,
          p[1] * scale,
          p[2] * scale,
        )
      })
      .collect(),
  )));
  placed(
    lua,
    a,
    "polyhedra.scad",
    "regular_polyhedron",
    hull,
    [r * 2.0, r * 2.0, r * 2.0],
  )
}

// ---------------------------------------------------------------------------
// Tripod mounts
// ---------------------------------------------------------------------------

fn tripod_mount(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let size = a.num_or("size", 40.0);
  let thickness = a.num_or("thickness", 6.0);
  // A plate with the standard 1/4-20 threaded hole through it.
  let node = ScadNode::Difference(vec![
    cube(size, size, thickness),
    cyl(6.35 / 2.0, thickness + 1.0, 24),
  ]);
  placed(
    lua,
    a,
    "tripod_mounts.scad",
    "tripod_mount",
    node,
    [size, size, thickness],
  )
}

fn manfrotto_rc2_plate(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let h = a.num_or("h", 10.0);
  // The RC2 plate is a 50 mm square with chamfered sides that lock into the
  // clamp, and the same 1/4-20 hole.
  let node = ScadNode::Difference(vec![
    ScadNode::Union(vec![
      at(cube(50.0, 42.0, h / 2.0), 0.0, 0.0, h / 4.0),
      ScadNode::Cylinder {
        r1: 22.0,
        r2: 25.0,
        h: (h / 2.0) as f32,
        segments: 4,
        center: true,
      },
    ]),
    cyl(6.35 / 2.0, h + 1.0, 24),
  ]);
  placed(
    lua,
    a,
    "tripod_mounts.scad",
    "manfrotto_rc2_plate",
    node,
    [50.0, 42.0, h],
  )
}

// ---------------------------------------------------------------------------
// Registration
// ---------------------------------------------------------------------------

const PART_PARAMS: &[&str] = &[
  "size",
  "h",
  "l",
  "w",
  "d",
  "od",
  "id",
  "length",
  "width",
  "height",
  "thickness",
  "thick",
  "wall",
  "strut",
  "base",
  "ang",
  "angle",
  "slide",
  "n",
  "r",
  "extents",
  "bracing",
  "clipthick",
  "shaft",
  "shaft_len",
  "depth",
  "trade_size",
  "tab",
  "tabwall",
  "max_bridge",
  "wirediam",
  "path",
  "cubes",
  "gaps",
  "segs",
  "offset",
  "inner",
  "arm_height",
  "layerheight",
  "foldangle",
  "hingegap",
  "neck_d",
  "thread_od",
  "support_d",
  "pitch",
  "tolerance",
  "neck_od",
  "texture",
  "support",
  "name",
  "nub_depth",
  "slot",
  "compression",
  "lev",
  "anchor",
  "spin",
  "orient",
  "fn",
  "cone",
];

pub fn register(lua: &Lua, bosl: &mlua::Table) -> LuaResult<()> {
  // --- NEMA steppers ---
  for (name, index) in [
    ("nema_motor_width", 0usize),
    ("nema_motor_plinth_height", 1),
    ("nema_motor_plinth_diam", 2),
    ("nema_motor_screw_spacing", 3),
    ("nema_motor_screw_size", 4),
    ("nema_motor_screw_depth", 5),
  ] {
    register_one(lua, bosl, name, &["size"], nema_field(index))?;
  }
  for (name, size) in [
    ("nema11_stepper", 11i64),
    ("nema14_stepper", 14),
    ("nema17_stepper", 17),
    ("nema23_stepper", 23),
    ("nema34_stepper", 34),
  ] {
    register_one(lua, bosl, name, PART_PARAMS, nema_stepper(size))?;
  }
  for (name, size) in [
    ("nema11_mount_holes", 11i64),
    ("nema14_mount_holes", 14),
    ("nema17_mount_holes", 17),
    ("nema23_mount_holes", 23),
    ("nema34_mount_holes", 34),
  ] {
    register_one(lua, bosl, name, PART_PARAMS, nema_mount_holes(size))?;
  }
  register_one(
    lua,
    bosl,
    "nema_mount_holes",
    PART_PARAMS,
    nema_mount_holes_generic,
  )?;

  // --- bearings ---
  register_one(lua, bosl, "lmXuu_info", &["size"], lmxuu_info)?;
  register_one(lua, bosl, "linear_bearing", PART_PARAMS, linear_bearing)?;
  register_one(
    lua,
    bosl,
    "linear_bearing_housing",
    PART_PARAMS,
    linear_bearing_housing,
  )?;
  register_one(
    lua,
    bosl,
    "ball_bearing_info",
    &["trade_size"],
    ball_bearing_info,
  )?;
  register_one(lua, bosl, "ball_bearing", PART_PARAMS, ball_bearing)?;

  // --- joiners ---
  for (name, f) in [
    (
      "dovetail",
      dovetail as fn(&Lua, &Args) -> LuaResult<LuaValue>,
    ),
    ("joiner", joiner),
    ("joiner_clear", joiner_clear),
    ("half_joiner", half_joiner),
    ("half_joiner2", half_joiner2),
    ("half_joiner_clear", half_joiner_clear),
    ("hirth", hirth),
    ("snap_pin", snap_pin),
    ("snap_pin_socket", snap_pin_socket),
    ("rabbit_clip", rabbit_clip),
    ("slider", slider),
    ("rail", rail),
    ("narrowing_strut", narrowing_strut),
    ("thinning_wall", thinning_wall),
    ("thinning_triangle", thinning_triangle),
    ("sparse_strut", sparse_strut),
    ("sparse_strut3d", sparse_strut3d),
    ("corrugated_wall", corrugated_wall),
    ("wiring", wiring),
    ("cubetruss", cubetruss),
    ("cubetruss_segment", cubetruss_segment),
    ("cubetruss_clip", cubetruss_clip),
    ("cubetruss_uclip", cubetruss_uclip),
    ("cubetruss_joiner", cubetruss_joiner),
    ("cubetruss_foot", cubetruss_foot),
    ("cubetruss_support", cubetruss_support),
    ("cubetruss_corner", cubetruss_corner),
    ("knuckle_hinge", knuckle_hinge),
    ("living_hinge_mask", living_hinge_mask),
    // BOSL2 renamed this; the old name builds the same slot.
    ("folding_hinge_mask", living_hinge_mask),
    ("generic_bottle_neck", generic_bottle_neck),
    ("generic_bottle_cap", generic_bottle_cap),
    ("regular_polyhedron", regular_polyhedron),
    ("tripod_mount", tripod_mount),
    ("manfrotto_rc2_plate", manfrotto_rc2_plate),
  ] {
    register_one(lua, bosl, name, PART_PARAMS, f)?;
  }
  register_one(
    lua,
    bosl,
    "cubetruss_dist",
    &["cubes", "gaps", "size", "strut"],
    cubetruss_dist,
  )?;
  register_one(lua, bosl, "hex_offsets", &["n", "d", "lev"], hex_offsets)?;
  register_one(lua, bosl, "hex_offset_ring", &["d", "lev"], hex_offset_ring)?;

  // --- standard bottle necks and caps ---
  for (name, neck_d, id, thread_od, height, support_d, pitch) in [
    ("pco1810_neck", 24.94, 21.74, 27.43, 21.0, 33.0, 3.18),
    ("pco1881_neck", 24.94, 21.74, 27.43, 17.0, 33.0, 2.7),
    ("sp_neck", 25.0, 21.4, 27.2, 17.0, 33.0, 3.2),
  ] {
    register_one(
      lua,
      bosl,
      name,
      PART_PARAMS,
      standard_neck(name, neck_d, id, thread_od, height, support_d, pitch),
    )?;
  }
  for (name, thread_od, height) in
    [("pco1810_cap", 27.43, 11.2), ("pco1881_cap", 27.43, 11.2)]
  {
    register_one(
      lua,
      bosl,
      name,
      PART_PARAMS,
      standard_cap(name, thread_od, height),
    )?;
  }
  for (name, cap_below, neck_above) in [
    ("bottle_adapter_neck_to_cap", false, true),
    ("bottle_adapter_cap_to_cap", true, false),
    ("bottle_adapter_neck_to_neck", false, false),
  ] {
    register_one(
      lua,
      bosl,
      name,
      PART_PARAMS,
      bottle_adapter(name, cap_below, neck_above),
    )?;
  }
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

  fn measure(code: &str) -> (f64, ([f32; 3], [f32; 3])) {
    let geoms = crate::lua_engine::execute_lua(code).unwrap();
    let node = geoms[0].scad.clone().unwrap();
    let m = crate::export::materialize_scad_manifold(&node);
    (m.volume(), m.bounding_box())
  }

  #[test]
  fn nema_dimensions_match_the_standard() {
    assert_eq!(eval::<f64>("return bosl.nema_motor_width(17)"), 42.3);
    assert_eq!(
      eval::<f64>("return bosl.nema_motor_screw_spacing(17)"),
      31.0
    );
    assert_eq!(eval::<f64>("return bosl.nema_motor_plinth_diam(17)"), 22.0);
  }

  #[test]
  fn an_unknown_nema_size_is_reported() {
    let lua = Lua::new();
    register_bosl(&lua).unwrap();
    let err = lua
      .load("return bosl.nema_motor_width(99)")
      .eval::<mlua::Value>()
      .unwrap_err()
      .to_string();
    assert!(err.contains("standard motor size"), "{err}");
  }

  #[test]
  fn a_stepper_motor_is_as_wide_as_its_size_says() {
    let (v, (lo, hi)) = measure("render(bosl.nema17_stepper())");
    assert!(v > 0.0);
    assert!((hi[0] - 42.3 / 2.0).abs() < 0.1, "{hi:?}");
    assert!((lo[0] + 42.3 / 2.0).abs() < 0.1, "{lo:?}");
    // The shaft stands proud of the body.
    assert!(hi[2] > 42.3 / 2.0, "{hi:?}");
  }

  #[test]
  fn mount_holes_are_spaced_to_match_the_motor() {
    let (v, (_, hi)) = measure("render(bosl.nema17_mount_holes())");
    assert!(v > 0.0);
    assert!((hi[0] - (31.0 + 3.0) / 2.0).abs() < 0.3, "{hi:?}");
  }

  #[test]
  fn bearing_tables_match_the_standard() {
    let info: Vec<f64> = eval("return bosl.lmXuu_info(8)");
    assert_eq!(info, vec![8.0, 15.0, 24.0]);
    let info: Vec<f64> = eval("return bosl.ball_bearing_info('608')");
    assert_eq!(info, vec![8.0, 22.0, 7.0]);
  }

  #[test]
  fn a_linear_bearing_is_a_tube_of_the_right_size() {
    let (v, (_, hi)) = measure("render(bosl.linear_bearing({size = 8}))");
    assert!(v > 0.0);
    assert!((hi[0] - 15.0 / 2.0).abs() < 0.2, "{hi:?}");
    assert!((hi[2] - 24.0 / 2.0).abs() < 0.1, "{hi:?}");
  }

  #[test]
  fn a_ball_bearing_is_hollow() {
    let (v, (_, hi)) =
      measure("render(bosl.ball_bearing({trade_size = '608'}))");
    let solid = std::f64::consts::PI * 11.0f64.powi(2) * 7.0;
    assert!(v > 0.0 && v < solid, "{v} vs {solid}");
    assert!((hi[0] - 11.0).abs() < 0.2, "{hi:?}");
  }

  #[test]
  fn the_joiners_all_build_something() {
    for call in [
      "bosl.dovetail()",
      "bosl.joiner()",
      "bosl.joiner_clear()",
      "bosl.half_joiner()",
      "bosl.half_joiner2()",
      "bosl.half_joiner_clear()",
      "bosl.hirth()",
      "bosl.snap_pin()",
      "bosl.snap_pin_socket()",
      "bosl.rabbit_clip()",
      "bosl.slider()",
      "bosl.rail()",
    ] {
      let (v, _) = measure(&format!("render({call})"));
      assert!(v > 0.0, "{call} produced nothing");
    }
  }

  #[test]
  fn the_walls_and_struts_all_build_something() {
    for call in [
      "bosl.narrowing_strut()",
      "bosl.thinning_wall()",
      "bosl.thinning_triangle()",
      "bosl.sparse_strut()",
      "bosl.sparse_strut3d()",
      "bosl.corrugated_wall()",
    ] {
      let (v, _) = measure(&format!("render({call})"));
      assert!(v > 0.0, "{call} produced nothing");
    }
  }

  #[test]
  fn a_truss_grows_with_the_extents_it_is_given() {
    let (one, _) = measure("render(bosl.cubetruss({extents = 1}))");
    let (three, _) = measure("render(bosl.cubetruss({extents = {3, 1, 1}}))");
    assert!(three > one * 2.5, "{three} vs {one}");
  }

  #[test]
  fn the_truss_fittings_all_build_something() {
    for call in [
      "bosl.cubetruss_segment()",
      "bosl.cubetruss_clip()",
      "bosl.cubetruss_joiner()",
      "bosl.cubetruss_foot()",
      "bosl.cubetruss_support()",
      "bosl.cubetruss_corner()",
    ] {
      let (v, _) = measure(&format!("render({call})"));
      assert!(v > 0.0, "{call} produced nothing");
    }
  }

  #[test]
  fn truss_distance_counts_the_cubes_and_the_gaps() {
    let d: f64 =
      eval("return bosl.cubetruss_dist({cubes = 3, gaps = 2, size = 30})");
    assert_eq!(d, 3.0 * 30.0 + 2.0 * 3.0);
  }

  #[test]
  fn hex_offsets_pack_a_bundle_of_wires() {
    let one: Vec<Vec<f64>> = eval("return bosl.hex_offsets(1)");
    assert_eq!(one, vec![vec![0.0, 0.0]]);
    let seven: Vec<Vec<f64>> = eval("return bosl.hex_offsets(7)");
    assert_eq!(seven.len(), 7);
    // The six round the centre are all one diameter out.
    for p in &seven[1..] {
      let r = (p[0] * p[0] + p[1] * p[1]).sqrt();
      assert!((r - 1.0).abs() < 1e-9, "{p:?}");
    }
  }

  #[test]
  fn wiring_runs_a_bundle_along_a_path() {
    let (v, (lo, hi)) = measure(
      "render(bosl.wiring({path = {{0,0,0},{50,0,0}}, n = 3,
                           wirediam = 2}))",
    );
    assert!(v > 0.0);
    assert!((hi[0] - lo[0] - 50.0).abs() < 1.0, "{lo:?} {hi:?}");
  }

  #[test]
  fn the_hinges_build_something() {
    for call in ["bosl.knuckle_hinge()", "bosl.living_hinge_mask()"] {
      let (v, _) = measure(&format!("render({call})"));
      assert!(v > 0.0, "{call} produced nothing");
    }
  }

  #[test]
  fn the_bottle_parts_build_something() {
    for call in [
      "bosl.generic_bottle_neck()",
      "bosl.generic_bottle_cap()",
      "bosl.pco1810_neck()",
      "bosl.pco1881_neck()",
      "bosl.sp_neck()",
      "bosl.pco1810_cap()",
      "bosl.pco1881_cap()",
      "bosl.bottle_adapter_neck_to_cap()",
      "bosl.bottle_adapter_cap_to_cap()",
      "bosl.bottle_adapter_neck_to_neck()",
    ] {
      let (v, _) = measure(&format!("render({call})"));
      assert!(v > 0.0, "{call} produced nothing");
    }
  }

  #[test]
  fn the_platonic_solids_have_the_volumes_they_should() {
    // A cube on a circumscribed radius of r has a side of 2r/sqrt(3).
    let (v, _) =
      measure("render(bosl.regular_polyhedron({name = 'cube', r = 10}))");
    let side = 20.0 / 3f64.sqrt();
    assert!((v - side.powi(3)).abs() / v < 0.01, "{v}");

    // A regular tetrahedron on radius r has volume 8r^3/(9*sqrt(3)).
    let (v, _) = measure(
      "render(bosl.regular_polyhedron({name = 'tetrahedron', r = 10}))",
    );
    let ideal = 8.0 * 1000.0 / (9.0 * 3f64.sqrt());
    assert!((v - ideal).abs() / ideal < 0.01, "{v} vs {ideal}");
  }

  #[test]
  fn every_platonic_solid_fits_its_radius() {
    for name in [
      "tetrahedron",
      "cube",
      "octahedron",
      "dodecahedron",
      "icosahedron",
    ] {
      let (v, (_, hi)) = measure(&format!(
        "render(bosl.regular_polyhedron({{name = '{name}', r = 10}}))"
      ));
      assert!(v > 0.0, "{name} produced nothing");
      assert!(hi[0] <= 10.01, "{name}: {hi:?}");
    }
  }

  #[test]
  fn an_unknown_polyhedron_is_reported() {
    let lua = Lua::new();
    register_bosl(&lua).unwrap();
    let err = lua
      .load("return bosl.regular_polyhedron({name = 'wrong'})")
      .eval::<mlua::Value>()
      .unwrap_err()
      .to_string();
    assert!(err.contains("tetrahedron"), "{err}");
  }

  #[test]
  fn the_tripod_mounts_have_a_quarter_inch_hole() {
    for call in ["bosl.tripod_mount()", "bosl.manfrotto_rc2_plate()"] {
      let (v, _) = measure(&format!("render({call})"));
      assert!(v > 0.0, "{call} produced nothing");
    }
    // The plate is drilled through, so it holds less than a solid one.
    let (plate, (lo, hi)) = measure("render(bosl.tripod_mount())");
    let solid = 40.0 * 40.0 * 6.0;
    assert!(plate < solid, "{plate} vs {solid}");
    assert!((hi[0] - lo[0] - 40.0).abs() < 0.1, "{lo:?} {hi:?}");
  }
}
