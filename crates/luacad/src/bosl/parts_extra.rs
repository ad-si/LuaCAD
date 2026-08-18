//! The remaining BOSL2 parts libraries: walls, hinges, stepper mounts,
//! bearings, wiring and hose fittings.
//!
//! These are all shapes with a standard behind them, or a printing trick
//! worth having on hand — a wall braced with diagonal struts uses a fraction
//! of the material a solid one does, a living hinge is a slot thin enough to
//! bend, and a NEMA mount has to match a motor nobody prints themselves.

use mlua::{Lua, Result as LuaResult, Value as LuaValue};

use crate::bosl::value::{Args, PureFn, Val, register_all, register_pure};
use crate::scad_export::ScadNode;

fn cube(w: f64, d: f64, h: f64, center: bool) -> ScadNode {
  ScadNode::Cube {
    w: w as f32,
    d: d as f32,
    h: h as f32,
    center,
  }
}

fn cyl(r: f64, h: f64, segments: u32, center: bool) -> ScadNode {
  ScadNode::Cylinder {
    r1: r as f32,
    r2: r as f32,
    h: h as f32,
    center,
    segments,
  }
}

fn moved(node: ScadNode, x: f64, y: f64, z: f64) -> ScadNode {
  ScadNode::Translate {
    x: x as f32,
    y: y as f32,
    z: z as f32,
    child: Box::new(node),
  }
}

fn turned(node: ScadNode, x: f64, y: f64, z: f64) -> ScadNode {
  ScadNode::Rotate {
    x: x as f32,
    y: y as f32,
    z: z as f32,
    child: Box::new(node),
  }
}

fn as_geometry(
  lua: &Lua,
  file: &'static str,
  name: &'static str,
  a: &Args,
  node: ScadNode,
) -> LuaResult<LuaValue> {
  let scad = crate::bosl::bosl_node_with_children(
    file,
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

// ---------------------------------------------------------------------------
// walls.scad
// ---------------------------------------------------------------------------

/// The diagonal struts that brace a sparse wall, as a flat outline.
///
/// The struts lean at `maxang` from vertical, which is what keeps every one
/// of them printable without support, and they alternate direction so the
/// bracing works whichever way the wall is pushed.
fn strut_field(
  h: f64,
  l: f64,
  thick: f64,
  maxang: f64,
  strut: f64,
) -> Vec<ScadNode> {
  let mut parts = vec![
    // The frame: a rail top and bottom, and a post at each end.
    moved(cube(l, thick, strut, true), 0.0, 0.0, h / 2.0 - strut / 2.0),
    moved(
      cube(l, thick, strut, true),
      0.0,
      0.0,
      -(h / 2.0 - strut / 2.0),
    ),
    moved(cube(strut, thick, h, true), l / 2.0 - strut / 2.0, 0.0, 0.0),
    moved(
      cube(strut, thick, h, true),
      -(l / 2.0 - strut / 2.0),
      0.0,
      0.0,
    ),
  ];

  // Each strut spans the full height, so its horizontal reach follows from
  // the lean angle; that reach sets how many fit along the wall.
  let reach = h * maxang.to_radians().tan();
  if reach <= 1e-9 {
    return parts;
  }
  let count = ((l / reach).ceil() as i64).max(1);
  let span = l / count as f64;
  let diagonal = (h * h + span * span).sqrt();
  let lean = span.atan2(h).to_degrees();
  for i in 0..count {
    let x = -l / 2.0 + span * (i as f64 + 0.5);
    // Alternating lean makes a zig-zag rather than a set of parallel bars.
    let sign = if i % 2 == 0 { 1.0 } else { -1.0 };
    parts.push(moved(
      turned(cube(strut, thick, diagonal, true), 0.0, sign * lean, 0.0),
      x,
      0.0,
      0.0,
    ));
  }
  parts
}

fn sparse_wall(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let h = a.num_or("h", 50.0);
  let l = a.num_or("l", 100.0);
  let thick = a.num_or("thick", 4.0);
  let maxang = a.num_or("maxang", 30.0);
  let strut = a.num_or("strut", 5.0);
  if h <= 0.0 || l <= 0.0 || thick <= 0.0 || strut <= 0.0 {
    return a.err("h, l, thick and strut must all be positive");
  }
  if !(0.0..90.0).contains(&maxang) {
    return a.err("maxang must be between 0 and 90");
  }
  let node = ScadNode::Intersection(vec![
    ScadNode::Union(strut_field(h, l, thick, maxang, strut)),
    // Trimmed to the wall's own outline, so a strut cannot poke out.
    cube(l, thick, h, true),
  ]);
  as_geometry(lua, "walls.scad", "sparse_wall", a, node)
}

/// The same bracing as a flat outline, for extruding to any thickness.
fn sparse_wall2d(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let size = a.sized("size", 2).unwrap_or_else(|| vec![50.0, 100.0]);
  let maxang = a.num_or("maxang", 30.0);
  let strut = a.num_or("strut", 5.0);
  if size.iter().any(|s| *s <= 0.0) || strut <= 0.0 {
    return a.err("size and strut must be positive");
  }
  // Built as a thin slab and flattened, so both forms brace identically.
  let node = ScadNode::Projection {
    cut: false,
    child: Box::new(ScadNode::Intersection(vec![
      ScadNode::Union(strut_field(size[0], size[1], 1.0, maxang, strut)),
      cube(size[1], 1.0, size[0], true),
    ])),
  };
  as_geometry(lua, "walls.scad", "sparse_wall2d", a, node)
}

/// A box with its inside braced rather than filled.
fn sparse_cuboid(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let size = a.sized("size", 3).unwrap_or_else(|| vec![50.0; 3]);
  let strut = a.num_or("strut", 5.0);
  let maxang = a.num_or("maxang", 30.0);
  if size.iter().any(|s| *s <= 0.0) || strut <= 0.0 {
    return a.err("size and strut must be positive");
  }
  // The bracing runs across the box in the direction it is thinnest, which
  // is the one that needs holding apart.
  let node = ScadNode::Intersection(vec![
    ScadNode::Union(strut_field(size[2], size[0], size[1], maxang, strut)),
    cube(size[0], size[1], size[2], true),
  ]);
  as_geometry(lua, "walls.scad", "sparse_cuboid", a, node)
}

/// A panel pierced with a hexagonal grid, with a solid frame round it.
fn hex_panel(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let size = a.sized("shape", 3).unwrap_or_else(|| vec![50.0, 50.0, 5.0]);
  let strut = a.num_or("strut", 2.0);
  let spacing = a.num_or("spacing", 15.0);
  let frame = a.num_or("frame", strut);
  if size.iter().any(|s| *s <= 0.0) {
    return a.err("shape must be positive in every direction");
  }
  if strut <= 0.0 || spacing <= strut {
    return a.err("spacing must be larger than strut, and both positive");
  }
  let (w, d, h) = (size[0], size[1], size[2]);

  // A hexagon whose flats are `strut` apart from its neighbour's leaves the
  // wall thickness the strut asks for.
  let across = spacing - strut;
  let r = across / 3f64.sqrt();
  let step_x = spacing;
  let step_y = spacing * 3f64.sqrt() / 2.0;
  let mut holes = Vec::new();
  let nx = (w / step_x).ceil() as i64 + 2;
  let ny = (d / step_y).ceil() as i64 + 2;
  for j in -ny..=ny {
    for i in -nx..=nx {
      // Every other row is offset by half a step, which is what makes the
      // grid hexagonal rather than square.
      let x = i as f64 * step_x + if j % 2 == 0 { 0.0 } else { step_x / 2.0 };
      let y = j as f64 * step_y;
      holes.push(moved(
        turned(cyl(r, h + 1.0, 6, true), 0.0, 0.0, 30.0),
        x,
        y,
        0.0,
      ));
    }
  }

  let inner = (w - 2.0 * frame).max(0.0);
  let inner_d = (d - 2.0 * frame).max(0.0);
  let node = ScadNode::Difference(vec![
    cube(w, d, h, true),
    // The holes are kept clear of the frame by trimming them to the inside.
    ScadNode::Intersection(vec![
      ScadNode::Union(holes),
      cube(inner, inner_d, h + 2.0, true),
    ]),
  ]);
  as_geometry(lua, "walls.scad", "hex_panel", a, node)
}

// ---------------------------------------------------------------------------
// hinges.scad
// ---------------------------------------------------------------------------

/// How far a snap sits from the fold, so it clears the hinge when closed.
fn snap_offset(
  thick: f64,
  snapdiam: f64,
  layerheight: f64,
  foldangle: f64,
  gap: f64,
) -> f64 {
  let half = (foldangle / 2.0).to_radians().tan();
  if half.abs() < 1e-9 {
    return 0.0;
  }
  (snapdiam / 2.0) / half + (thick - 2.0 * layerheight) / half + gap / 2.0
}

/// The peg half of a snap that holds a folded hinge shut.
fn snap_lock(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let thick = a.need_num("thick")?;
  let snaplen = a.num_or("snaplen", 5.0);
  let snapdiam = a.num_or("snapdiam", 5.0);
  let layerheight = a.num_or("layerheight", 0.2);
  let foldangle = a.num_or("foldangle", 90.0);
  let gap = a.num("hingegap").unwrap_or(layerheight);
  if thick <= 0.0 || snaplen <= 0.0 || snapdiam <= 0.0 {
    return a.err("thick, snaplen and snapdiam must be positive");
  }
  let y = snap_offset(thick, snapdiam, layerheight, foldangle, gap);
  // A rounded peg on a stalk: the round is what lets it click past the lip.
  let node = moved(
    ScadNode::Union(vec![
      turned(cyl(snapdiam / 2.0, snaplen, 24, true), 0.0, 90.0, 0.0),
      cube(snaplen, snapdiam, thick, true),
    ]),
    0.0,
    y,
    0.0,
  );
  as_geometry(lua, "hinges.scad", "snap_lock", a, node)
}

/// The socket half, which the peg clicks into.
fn snap_socket(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let thick = a.need_num("thick")?;
  let snaplen = a.num_or("snaplen", 5.0);
  let snapdiam = a.num_or("snapdiam", 5.0);
  let layerheight = a.num_or("layerheight", 0.2);
  let foldangle = a.num_or("foldangle", 90.0);
  let gap = a.num("hingegap").unwrap_or(layerheight);
  if thick <= 0.0 || snaplen <= 0.0 || snapdiam <= 0.0 {
    return a.err("thick, snaplen and snapdiam must be positive");
  }
  let slop = crate::bosl::get_slop();
  let y = snap_offset(thick, snapdiam, layerheight, foldangle, gap);
  let node = moved(
    ScadNode::Difference(vec![
      cube(snaplen + 2.0 * snapdiam, snapdiam * 2.0, thick, true),
      turned(
        cyl(snapdiam / 2.0 + slop, snaplen + 0.2, 24, true),
        0.0,
        90.0,
        0.0,
      ),
    ]),
    0.0,
    -y,
    0.0,
  );
  as_geometry(lua, "hinges.scad", "snap_socket", a, node)
}

/// Cut the hinges and snaps into a flat panel.
///
/// The panel is handed in as `p`, and comes back with a slot at each hinge
/// line and a pocket at each snap — the whole fold-up pattern in one step.
fn apply_folding_hinges_and_snaps(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let thick = a.need_num("thick")?;
  let Some(LuaValue::UserData(ud)) = a.raw("p") else {
    return a.err("p must be the panel to cut the hinges into");
  };
  let Ok(panel) = ud.borrow::<crate::geometry::CsgGeometry>() else {
    return a.err("p must be a solid");
  };
  let Some(base) = panel.scad.clone() else {
    return a.err("p has nothing in it");
  };
  let layerheight = a.num_or("layerheight", 0.2);
  let foldangle = a.num_or("foldangle", 90.0);
  let gap =
    a.num("hingegap").unwrap_or(layerheight) + 2.0 * crate::bosl::get_slop();

  // Each hinge is [length, position, angle]; each snap and socket is a
  // position and an angle.
  let read_list = |name: &str| -> Vec<Vec<f64>> {
    a.val(name)
      .and_then(|v| v.as_list().map(|s| s.to_vec()))
      .map(|items| {
        items
          .iter()
          .filter_map(|i| {
            let rows = i.as_list()?;
            let mut out = Vec::new();
            for r in rows {
              match r {
                Val::Num(n) => out.push(*n),
                other => out.extend(other.as_vec().unwrap_or_default()),
              }
            }
            Some(out)
          })
          .collect()
      })
      .unwrap_or_default()
  };

  let mut cuts: Vec<ScadNode> = Vec::new();
  for hinge in read_list("hinges") {
    if hinge.len() < 3 {
      return a.err("each hinge is a length, a position and an angle");
    }
    let (len, x, y) = (hinge[0], hinge[1], hinge[2]);
    let ang = *hinge.get(3).unwrap_or(&0.0);
    // The slot is left one layer thin at its base, which is the bit that
    // actually bends.
    let depth = thick - layerheight;
    let width =
      gap + 2.0 * depth / (foldangle / 2.0).to_radians().tan().max(1e-9);
    cuts.push(moved(
      turned(cube(len, width, depth, true), 0.0, 0.0, ang),
      x,
      y,
      thick / 2.0 - depth / 2.0 + 0.001,
    ));
  }
  for socket in read_list("sockets") {
    if socket.len() < 2 {
      return a.err("each socket is a position and an angle");
    }
    let snapdiam = a.num_or("snapdiam", 5.0);
    let snaplen = a.num_or("snaplen", 5.0);
    cuts.push(moved(
      turned(
        cube(snaplen, snapdiam, thick + 0.2, true),
        0.0,
        0.0,
        *socket.get(2).unwrap_or(&0.0),
      ),
      socket[0],
      socket[1],
      0.0,
    ));
  }

  let node = if cuts.is_empty() {
    base
  } else {
    ScadNode::Difference(vec![base, ScadNode::Union(cuts)])
  };
  as_geometry(
    lua,
    "hinges.scad",
    "apply_folding_hinges_and_snaps",
    a,
    node,
  )
}

// ---------------------------------------------------------------------------
// nema_steppers.scad
// ---------------------------------------------------------------------------

/// The dimensions of a NEMA stepper, by frame size.
///
/// In order: body width, plinth height, plinth diameter, screw spacing,
/// screw size, screw depth and shaft diameter.
fn nema_info(size: i64) -> Option<[f64; 7]> {
  Some(match size {
    6 => [14.0, 1.50, 11.0, 11.50, 1.6, 2.5, 4.00],
    8 => [20.3, 1.50, 16.0, 15.40, 2.0, 2.5, 4.00],
    11 => [28.2, 1.50, 22.0, 23.11, 2.6, 3.0, 5.00],
    14 => [35.2, 2.00, 22.0, 26.00, 3.0, 4.5, 5.00],
    17 => [42.3, 2.00, 22.0, 31.00, 3.0, 4.5, 5.00],
    23 => [57.0, 1.60, 38.1, 47.00, 5.1, 4.8, 6.35],
    34 => [86.0, 2.00, 73.0, 69.60, 6.5, 10.0, 14.00],
    42 => [110.0, 1.50, 55.5, 88.90, 8.5, 12.7, 19.00],
    _ => return None,
  })
}

const NEMA_SIZES: &[i64] = &[6, 8, 11, 14, 17, 23, 34, 42];

fn nema_motor_info(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let size = a.need_num("size")?.round() as i64;
  match nema_info(size) {
    Some(info) => Val::vec(info).to_lua(lua),
    None => a.err(format!(
      "no NEMA {size}; the sizes are {}",
      NEMA_SIZES
        .iter()
        .map(|s| s.to_string())
        .collect::<Vec<_>>()
        .join(", ")
    )),
  }
}

fn nema_stepper_motor(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let size = a.num_or("size", 17.0).round() as i64;
  let Some(info) = nema_info(size) else {
    return a.err(format!("no NEMA {size}"));
  };
  let h = a.num_or("h", 24.0);
  let shaft_len = a.num_or("shaft_len", 20.0);
  let [width, plinth_h, plinth_d, spacing, screw, _, shaft] = info;

  let mut parts = vec![
    moved(cube(width, width, h, true), 0.0, 0.0, -h / 2.0),
    // The raised boss the motor centres on, and the shaft through it.
    cyl(plinth_d / 2.0, plinth_h, 32, false),
    cyl(shaft / 2.0, shaft_len, 24, false),
  ];
  // Four mounting bosses, one at each corner of the screw square.
  for (sx, sy) in [(1.0, 1.0), (1.0, -1.0), (-1.0, 1.0), (-1.0, -1.0)] {
    parts.push(moved(
      cyl(screw / 2.0, plinth_h, 16, false),
      sx * spacing / 2.0,
      sy * spacing / 2.0,
      0.0,
    ));
  }
  as_geometry(
    lua,
    "nema_steppers.scad",
    "nema_stepper_motor",
    a,
    ScadNode::Union(parts),
  )
}

/// The pocket and screw holes a NEMA motor mounts into.
fn nema_mount_mask(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let size = a.num_or("size", 17.0).round() as i64;
  let Some(info) = nema_info(size) else {
    return a.err(format!("no NEMA {size}"));
  };
  let depth = a.num_or("depth", 5.0);
  let l = a.num_or("l", 5.0);
  let slop = crate::bosl::get_slop();
  let [_, _, plinth_d, spacing, screw, _, _] = info;

  let mut parts = vec![cyl(plinth_d / 2.0 + slop, depth * 2.0, 32, true)];
  for (sx, sy) in [(1.0, 1.0), (1.0, -1.0), (-1.0, 1.0), (-1.0, -1.0)] {
    // Slotted rather than round, so the motor can be shifted to tension a
    // belt after it is bolted down.
    parts.push(moved(
      ScadNode::Union(vec![
        cyl(screw / 2.0 + slop, depth * 2.0, 16, true),
        moved(
          cube(l, screw + 2.0 * slop, depth * 2.0, true),
          l / 2.0 * sx,
          0.0,
          0.0,
        ),
      ]),
      sx * spacing / 2.0,
      sy * spacing / 2.0,
      0.0,
    ));
  }
  as_geometry(
    lua,
    "nema_steppers.scad",
    "nema_mount_mask",
    a,
    ScadNode::Union(parts),
  )
}

// ---------------------------------------------------------------------------
// linear_bearings.scad
// ---------------------------------------------------------------------------

/// An LM-UU linear bearing's outside diameter and length, by bore.
fn lm_info(size: i64) -> Option<(f64, f64)> {
  Some(match size {
    4 => (8.0, 12.0),
    5 => (10.0, 15.0),
    6 => (12.0, 19.0),
    8 => (15.0, 24.0),
    10 => (19.0, 29.0),
    12 => (21.0, 30.0),
    13 => (23.0, 32.0),
    16 => (28.0, 37.0),
    20 => (32.0, 42.0),
    25 => (40.0, 59.0),
    30 => (45.0, 64.0),
    35 => (52.0, 70.0),
    40 => (60.0, 80.0),
    50 => (80.0, 100.0),
    60 => (90.0, 110.0),
    80 => (120.0, 140.0),
    100 => (150.0, 175.0),
    _ => return None,
  })
}

fn lmxuu_bearing(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let size = a.num_or("size", 8.0).round() as i64;
  let Some((od, len)) = lm_info(size) else {
    return a.err(format!("no LM{size}UU bearing in the table"));
  };
  let node = ScadNode::Difference(vec![
    cyl(od / 2.0, len, 48, true),
    cyl(size as f64 / 2.0, len + 0.2, 48, true),
  ]);
  as_geometry(lua, "linear_bearings.scad", "lmXuu_bearing", a, node)
}

/// A clamp that holds an LM-UU bearing, with a tab to bolt it down by.
fn lmxuu_housing(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let size = a.num_or("size", 8.0).round() as i64;
  let Some((od, len)) = lm_info(size) else {
    return a.err(format!("no LM{size}UU bearing in the table"));
  };
  let tab = a.num_or("tab", 7.0);
  let gap = a.num_or("gap", 5.0);
  let wall = a.num_or("wall", 3.0);
  let tabwall = a.num_or("tabwall", 5.0);
  let screwsize = a.num_or("screwsize", 3.0);
  let slop = crate::bosl::get_slop();
  let outer = od / 2.0 + wall;

  let node = ScadNode::Difference(vec![
    ScadNode::Union(vec![
      cyl(outer, len, 48, true),
      // The two ears the clamping screw pulls together.
      moved(
        cube(tabwall * 2.0 + gap, tab, len, true),
        0.0,
        outer + tab / 2.0,
        0.0,
      ),
    ]),
    // The bore, the slit that lets it close, and the screw through the ears.
    cyl(od / 2.0 + slop, len + 0.2, 48, true),
    moved(
      cube(gap, outer + tab, len + 0.2, true),
      0.0,
      outer / 2.0,
      0.0,
    ),
    moved(
      turned(
        cyl(screwsize / 2.0, tabwall * 2.0 + gap + 1.0, 24, true),
        0.0,
        90.0,
        0.0,
      ),
      0.0,
      outer + tab / 2.0,
      0.0,
    ),
  ]);
  as_geometry(lua, "linear_bearings.scad", "lmXuu_housing", a, node)
}

// ---------------------------------------------------------------------------
// wiring.scad and modular_hose.scad
// ---------------------------------------------------------------------------

/// A bundle of wires following a path.
///
/// The wires are packed into rings around the centre of the bundle, which is
/// how a real loom sits, and each one is swept along the path.
fn wire_bundle(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let Some(path) = a.points3("path") else {
    return a.err("path must be a list of points");
  };
  if path.len() < 2 {
    return a.err("path needs at least two points");
  }
  let wires = a.num_or("wires", 1.0).round().max(1.0) as usize;
  let wirediam = a.num_or("wirediam", 2.0);
  if wirediam <= 0.0 {
    return a.err("wirediam must be positive");
  }

  // Ring 0 holds one wire, and each ring out holds six more than the last.
  let mut offsets: Vec<[f64; 2]> = vec![[0.0, 0.0]];
  let mut ring = 1usize;
  while offsets.len() < wires {
    let count = 6 * ring;
    let r = wirediam * ring as f64;
    for k in 0..count {
      if offsets.len() >= wires {
        break;
      }
      let ang = std::f64::consts::TAU * k as f64 / count as f64;
      offsets.push([r * ang.cos(), r * ang.sin()]);
    }
    ring += 1;
  }

  // Each wire is the path itself, shifted across the bundle and swept.
  let mut parts = Vec::with_capacity(wires);
  for off in &offsets {
    let mut segments = Vec::new();
    for w in path.windows(2) {
      let d = [w[1][0] - w[0][0], w[1][1] - w[0][1], w[1][2] - w[0][2]];
      let len = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
      if len < 1e-9 {
        continue;
      }
      segments.push(moved(
        turned(
          moved(cyl(wirediam / 2.0, len, 16, false), off[0], off[1], 0.0),
          0.0,
          (d[2] / len).clamp(-1.0, 1.0).acos().to_degrees(),
          d[1].atan2(d[0]).to_degrees(),
        ),
        w[0][0],
        w[0][1],
        w[0][2],
      ));
    }
    parts.push(ScadNode::Union(segments));
  }
  as_geometry(lua, "wiring.scad", "wire_bundle", a, ScadNode::Union(parts))
}

/// A modular coolant hose segment: a ball at one end, a socket at the other.
fn modular_hose(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let size = a.need_num("size")?;
  let clearance = a.num_or("clearance", 0.0);
  let table: &[(f64, f64, f64)] =
    &[(0.25, 3.4, 6.4), (0.5, 6.5, 11.4), (0.75, 9.5, 16.4)];
  let Some((_, bore, outer)) =
    table.iter().find(|(s, _, _)| (s - size).abs() < 1e-9)
  else {
    return a.err("size must be 1/4, 1/2 or 3/4");
  };
  let kind = a.string("type").unwrap_or_else(|| "socket".to_string());
  let waist = a.num_or("waist_len", outer * 0.6);
  let ball_r = outer / 2.0;

  let ball = ScadNode::Sphere {
    r: (ball_r + clearance) as f32,
    segments: 48,
  };
  let neck = cyl(bore + 1.0, waist, 32, false);
  let node = match kind.as_str() {
    // The ball end is a sphere on a stalk, bored through for the coolant.
    "ball" => ScadNode::Difference(vec![
      ScadNode::Union(vec![neck, moved(ball, 0.0, 0.0, waist + ball_r)]),
      cyl(*bore, waist + 2.0 * ball_r + 1.0, 32, false),
    ]),
    // The socket is the same stalk with the ball taken out of it.
    "socket" => ScadNode::Difference(vec![
      cyl(ball_r + 2.0, waist + ball_r, 48, false),
      moved(ball, 0.0, 0.0, waist + ball_r),
      cyl(*bore, waist + ball_r + 1.0, 32, false),
    ]),
    other => {
      return a.err(format!(
        "unknown hose end '{other}'; use \"ball\" or \"socket\""
      ));
    }
  };
  as_geometry(lua, "modular_hose.scad", "modular_hose", a, node)
}

/// A screw cap for an SP-series bottle neck.
fn sp_cap(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let diam = a.need_num("diam")?;
  let sp_type = a.num_or("type", 400.0) as i64;
  let wall = a.num_or("wall", 1.5);
  // The real thread diameter comes from the same table sp_diameter reads.
  let Some(thread) = crate::bosl::metric::sp_thread_diameter(diam, sp_type)
  else {
    return a.err(format!("SP{sp_type} has no {diam} closure"));
  };
  if wall <= 0.0 {
    return a.err("wall must be positive");
  }
  let h = a.num_or("height", thread * 0.4);
  let node = ScadNode::Difference(vec![
    cyl(thread / 2.0 + wall, h, 64, false),
    moved(cyl(thread / 2.0, h, 64, false), 0.0, 0.0, wall),
  ]);
  as_geometry(lua, "bottlecaps.scad", "sp_cap", a, node)
}

pub fn register(lua: &Lua, bosl: &mlua::Table) -> LuaResult<()> {
  register_pure(
    lua,
    bosl,
    "sparse_wall",
    &[
      "h",
      "l",
      "thick",
      "maxang",
      "strut",
      "max_bridge",
      "anchor",
      "spin",
      "orient",
    ],
    sparse_wall,
  )?;
  register_pure(
    lua,
    bosl,
    "sparse_wall2d",
    &["size", "maxang", "strut", "max_bridge", "anchor", "spin"],
    sparse_wall2d,
  )?;
  register_pure(
    lua,
    bosl,
    "sparse_cuboid",
    &[
      "size",
      "dir",
      "strut",
      "maxang",
      "max_bridge",
      "chamfer",
      "rounding",
      "edges",
      "except",
      "except_edges",
      "trimcorners",
      "anchor",
      "spin",
      "orient",
    ],
    sparse_cuboid,
  )?;
  register_pure(
    lua,
    bosl,
    "hex_panel",
    &[
      "shape",
      "strut",
      "spacing",
      "frame",
      "bevel_frame",
      "h",
      "height",
      "l",
      "length",
      "anchor",
      "spin",
      "orient",
    ],
    hex_panel,
  )?;

  register_pure(
    lua,
    bosl,
    "snap_lock",
    &[
      "thick",
      "snaplen",
      "snapdiam",
      "layerheight",
      "foldangle",
      "hingegap",
      "anchor",
      "spin",
      "orient",
    ],
    snap_lock,
  )?;
  register_pure(
    lua,
    bosl,
    "snap_socket",
    &[
      "thick",
      "snaplen",
      "snapdiam",
      "layerheight",
      "foldangle",
      "hingegap",
      "anchor",
      "spin",
      "orient",
    ],
    snap_socket,
  )?;
  register_pure(
    lua,
    bosl,
    "apply_folding_hinges_and_snaps",
    &[
      "thick",
      "foldangle",
      "hinges",
      "snaps",
      "sockets",
      "snaplen",
      "snapdiam",
      "hingegap",
      "layerheight",
      "p",
    ],
    apply_folding_hinges_and_snaps,
  )?;

  register_pure(
    lua,
    bosl,
    "nema_stepper_motor",
    &[
      "size",
      "h",
      "shaft_len",
      "details",
      "atype",
      "anchor",
      "spin",
      "orient",
    ],
    nema_stepper_motor,
  )?;
  register_pure(
    lua,
    bosl,
    "nema_mount_mask",
    &["size", "depth", "l", "atype", "anchor", "spin", "orient"],
    nema_mount_mask,
  )?;
  register_pure(
    lua,
    bosl,
    "lmXuu_bearing",
    &["size", "anchor", "spin", "orient"],
    lmxuu_bearing,
  )?;
  register_pure(
    lua,
    bosl,
    "lmXuu_housing",
    &[
      "size",
      "tab",
      "gap",
      "wall",
      "tabwall",
      "screwsize",
      "anchor",
      "spin",
      "orient",
    ],
    lmxuu_housing,
  )?;
  register_pure(
    lua,
    bosl,
    "wire_bundle",
    &[
      "path",
      "wires",
      "wirediam",
      "rounding",
      "wirenum",
      "corner_steps",
    ],
    wire_bundle,
  )?;
  register_pure(
    lua,
    bosl,
    "modular_hose",
    &[
      "size",
      "type",
      "clearance",
      "waist_len",
      "anchor",
      "spin",
      "orient",
    ],
    modular_hose,
  )?;
  register_pure(
    lua,
    bosl,
    "sp_cap",
    &[
      "diam", "type", "wall", "style", "top_adj", "bot_adj", "texture",
      "height", "anchor", "spin", "orient",
    ],
    sp_cap,
  )?;

  register_all(
    lua,
    bosl,
    &[("nema_motor_info", &["size"], nema_motor_info as PureFn)],
  )
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn every_nema_size_has_a_full_row() {
    for size in NEMA_SIZES {
      let info = nema_info(*size).unwrap_or_else(|| panic!("NEMA {size}"));
      assert!(info.iter().all(|v| *v > 0.0), "NEMA {size}: {info:?}");
      // The screws have to sit inside the body they bolt through.
      assert!(
        info[3] < info[0],
        "NEMA {size} screw spacing exceeds its body"
      );
    }
    assert!(nema_info(99).is_none());
  }

  #[test]
  fn a_bearing_is_always_wider_than_its_bore() {
    for size in [4, 8, 12, 25, 100] {
      let (od, len) = lm_info(size).unwrap();
      assert!(od > size as f64, "LM{size}UU");
      assert!(len > 0.0);
    }
    assert!(lm_info(7).is_none());
  }

  #[test]
  fn a_snap_sits_further_out_on_a_thicker_panel() {
    let near = snap_offset(2.0, 5.0, 0.2, 90.0, 0.2);
    let far = snap_offset(6.0, 5.0, 0.2, 90.0, 0.2);
    assert!(far > near, "{near} vs {far}");
  }

  #[test]
  fn a_sparse_wall_braces_with_more_struts_as_it_gets_longer() {
    let short = strut_field(50.0, 50.0, 4.0, 30.0, 5.0).len();
    let long = strut_field(50.0, 200.0, 4.0, 30.0, 5.0).len();
    assert!(long > short, "{short} vs {long}");
    // Four frame pieces are always there, whatever the length.
    assert!(short >= 4);
  }
}
