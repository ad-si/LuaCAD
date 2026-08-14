//! BOSL2's `partitions.scad`, `masks2d.scad` and `masks3d.scad`.
//!
//! Partitions cut a shape in half with a plane. Masks are the solids you
//! subtract from an edge or a corner to round or chamfer it, and the 2D ones
//! are the profiles those masks are swept from.

use mlua::{Lua, Result as LuaResult, Value as LuaValue};

use crate::bosl::attach::{Attachable, Geom, reorient};
use crate::bosl::value::{Args, Val, v3};
use crate::bosl::vecmath::Mat4;
use crate::bosl::vnf::{Vnf, arc_pts};
use crate::geometry::{CsgGeometry, CsgSketch};
use crate::scad_export::ScadNode;

const EPS: f64 = 1e-9;

/// Wrap a built solid as a BOSL2 call.
fn as_geometry(
  lua: &Lua,
  function: &'static str,
  native: ScadNode,
) -> LuaResult<LuaValue> {
  let scad = crate::bosl::bosl_node_with_children(
    "std.scad",
    function,
    String::new(),
    vec![],
    Some(native),
  );
  Ok(LuaValue::UserData(lua.create_userdata(CsgGeometry {
    name: None,
    mesh: None,
    color: None,
    scad: Some(scad),
  })?))
}

/// Wrap a built outline as a BOSL2 call.
fn as_sketch(
  lua: &Lua,
  function: &'static str,
  native: ScadNode,
) -> LuaResult<LuaValue> {
  let scad = crate::bosl::bosl_node_with_children(
    "std.scad",
    function,
    String::new(),
    vec![],
    Some(native),
  );
  Ok(LuaValue::UserData(lua.create_userdata(CsgSketch {
    #[cfg(feature = "csgrs")]
    sketch: crate::geometry::empty_sketch(),
    #[cfg(not(feature = "csgrs"))]
    sketch: (),
    color: None,
    scad: Some(scad),
  })?))
}

/// The shape a partition was given, and whether it was flat.
fn read_child(a: &Args) -> LuaResult<(ScadNode, bool)> {
  match a.raw("p") {
    Some(LuaValue::UserData(ud)) => {
      if let Ok(g) = ud.borrow::<CsgGeometry>() {
        return Ok((g.scad.clone().unwrap_or(ScadNode::Union(vec![])), false));
      }
      if let Ok(s) = ud.borrow::<CsgSketch>() {
        return Ok((s.scad.clone().unwrap_or(ScadNode::Union(vec![])), true));
      }
      a.err("p must be a shape")
    }
    _ => a.err("p must be a shape"),
  }
}

// ---------------------------------------------------------------------------
// Partitions
// ---------------------------------------------------------------------------

/// Keep the part of a shape on one side of a plane.
fn half_along(
  lua: &Lua,
  a: &Args,
  function: &'static str,
  dir: [f64; 3],
  offset: f64,
) -> LuaResult<LuaValue> {
  let (child, planar) = read_child(a)?;
  let s = a.num_or("s", 100.0);
  // The half-space is a box as big as the shape could be, pushed so its face
  // lies on the cutting plane.
  let keep = ScadNode::Cube {
    w: (s * 2.0) as f32,
    d: (s * 2.0) as f32,
    h: (s * 2.0) as f32,
    center: true,
  };
  let placed = crate::bosl::attach::transform(
    keep,
    Mat4::translate([
      dir[0] * (s + offset),
      dir[1] * (s + offset),
      dir[2] * (s + offset),
    ]),
  );
  let node = ScadNode::Intersection(vec![child, placed]);
  if planar {
    as_sketch(lua, function, node)
  } else {
    as_geometry(lua, function, node)
  }
}

fn half_of(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let v = a.val("v").and_then(|v| v.as_vec()).unwrap_or_default();
  // A 4-vector is a plane; its first three components are the normal and the
  // fourth is how far along it the plane sits.
  let (dir, offset) = if v.len() == 4 {
    let n = crate::bosl::vecmath::unit_or([v[0], v[1], v[2]], [0.0, 0.0, 1.0]);
    (n, v[3])
  } else {
    let n = crate::bosl::vecmath::unit_or(v3(&v), [0.0, 0.0, 1.0]);
    let cp = a
      .val("cp")
      .and_then(|c| c.as_vec())
      .map(|c| v3(&c))
      .unwrap_or([0.0; 3]);
    (n, n[0] * cp[0] + n[1] * cp[1] + n[2] * cp[2])
  };
  half_along(lua, a, "half_of", dir, offset)
}

fn axis_half(
  function: &'static str,
  dir: [f64; 3],
  param: &'static str,
) -> impl Fn(&Lua, &Args) -> LuaResult<LuaValue> {
  move |lua, a| {
    let offset = a.num_or(param, 0.0);
    // The offset is measured along the axis, not along the keep direction.
    let along = dir[0] * offset + dir[1] * offset + dir[2] * offset;
    half_along(lua, a, function, dir, along.abs() * along.signum())
  }
}

/// A jigsaw or dovetail profile, as the path a partition is cut along.
fn partition_cutpath(
  l: f64,
  h: f64,
  cutsize: [f64; 2],
  style: &str,
  gap: f64,
) -> Vec<[f64; 2]> {
  let period = cutsize[0].max(EPS);
  let amp = cutsize[1];
  let cycles = (l / period).ceil().max(1.0) as usize;
  let steps = 16usize;
  let mut path: Vec<[f64; 2]> = Vec::new();
  for c in 0..cycles {
    for s in 0..steps {
      let t = (c as f64 + s as f64 / steps as f64) * period;
      let u = (t / period).fract();
      let y = match style {
        // A flat join, with only the gap holding the halves apart.
        "flat" => 0.0,
        // A square-toothed join.
        "sqwave" => {
          if u < 0.5 {
            amp
          } else {
            -amp
          }
        }
        // A dovetail widens as it goes, so the halves cannot pull apart.
        "dovetail" => {
          let v = (u * 4.0).rem_euclid(4.0);
          amp
            * match v {
              v if v < 1.0 => -1.0 + v * 2.0,
              v if v < 2.0 => 1.0,
              v if v < 3.0 => 1.0 - (v - 2.0) * 2.0,
              _ => -1.0,
            }
        }
        // The default jigsaw is a smooth wave with rounded tabs.
        _ => amp * (std::f64::consts::TAU * u).sin(),
      };
      path.push([t - l / 2.0, y + gap / 2.0]);
    }
  }
  path.push([l / 2.0, gap / 2.0]);
  // Close the profile downward so it can be extruded into a solid.
  path.push([l / 2.0, -h]);
  path.push([-l / 2.0, -h]);
  path
}

fn read_cutsize(a: &Args) -> [f64; 2] {
  match a.val("cutsize") {
    Some(Val::Num(c)) => [c * 2.0, c],
    Some(other) => match other.as_vec() {
      Some(v) if v.len() >= 2 => [v[0], v[1]],
      _ => [20.0, 10.0],
    },
    None => [20.0, 10.0],
  }
}

fn partition_mask(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let l = a.num_or("l", 100.0);
  let w = a.num_or("w", 100.0);
  let h = a.num_or("h", 100.0);
  let gap = a.num_or("gap", 0.0);
  let style = a.string("cutpath").unwrap_or_else(|| "jigsaw".to_string());
  let inverse = a.bool_or("inverse", false);

  let path = partition_cutpath(l, h, read_cutsize(a), &style, gap);
  let path = if inverse {
    path.iter().map(|p| [p[0], -p[1]]).collect()
  } else {
    path
  };
  let node = ScadNode::LinearExtrude {
    height: w as f32,
    center: true,
    twist: 0.0,
    slices: 1,
    scale: 1.0,
    child: Box::new(crate::bosl::shapes2d::path_node(&path)),
  };
  // The profile is drawn in XZ, so it stands up rather than lying flat.
  let node = crate::bosl::attach::transform(node, Mat4::xrot(90.0));
  as_geometry(lua, "partition_mask", node)
}

fn partition_cut_mask(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let l = a.num_or("l", 100.0);
  let w = a.num_or("w", 100.0);
  let h = a.num_or("h", 100.0);
  let gap = a.num_or("gap", 0.0);
  let style = a.string("cutpath").unwrap_or_else(|| "jigsaw".to_string());
  let cutsize = read_cutsize(a);

  // The cut itself is a thin sheet along the join, for splitting a shape in
  // two without leaving a gap.
  let path = partition_cutpath(l, h, cutsize, &style, gap);
  let thin: Vec<[f64; 2]> = path
    .iter()
    .map(|p| [p[0], p[1]])
    .chain(path.iter().rev().map(|p| [p[0], p[1] - 0.01]))
    .collect();
  let node = ScadNode::LinearExtrude {
    height: w as f32,
    center: true,
    twist: 0.0,
    slices: 1,
    scale: 1.0,
    child: Box::new(crate::bosl::shapes2d::path_node(&thin)),
  };
  as_geometry(
    lua,
    "partition_cut_mask",
    crate::bosl::attach::transform(node, Mat4::xrot(90.0)),
  )
}

fn partition(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let (child, _) = read_child(a)?;
  let l = a.num_or("size", 100.0);
  let spread = a.num_or("spread", 10.0);
  let gap = a.num_or("gap", 0.0);
  let style = a.string("cutpath").unwrap_or_else(|| "jigsaw".to_string());
  let cutsize = read_cutsize(a);
  let h = l;
  let path = partition_cutpath(l, h, cutsize, &style, gap);

  let sheet = |flip: bool| -> ScadNode {
    let p: Vec<[f64; 2]> = if flip {
      path.iter().map(|q| [q[0], -q[1]]).collect()
    } else {
      path.clone()
    };
    let node = ScadNode::LinearExtrude {
      height: (l * 2.0) as f32,
      center: true,
      twist: 0.0,
      slices: 1,
      scale: 1.0,
      child: Box::new(crate::bosl::shapes2d::path_node(&p)),
    };
    crate::bosl::attach::transform(node, Mat4::xrot(90.0))
  };

  // The two halves are cut apart along the join and then moved aside.
  let left = ScadNode::Intersection(vec![child.clone(), sheet(false)]);
  let right = ScadNode::Difference(vec![child, sheet(false)]);
  let node = ScadNode::Union(vec![
    crate::bosl::attach::transform(
      left,
      Mat4::translate([0.0, -spread / 2.0, 0.0]),
    ),
    crate::bosl::attach::transform(
      right,
      Mat4::translate([0.0, spread / 2.0, 0.0]),
    ),
  ]);
  as_geometry(lua, "partition", node)
}

// ---------------------------------------------------------------------------
// 2D masks
// ---------------------------------------------------------------------------

/// The corner region a 2D mask fills, given the profile that cuts across it.
///
/// Every 2D mask is the same square corner with a different curve taken out
/// of it, so they share this: the profile runs from the X leg to the Y leg
/// and the corner closes it.
fn corner_mask_path(profile: &[[f64; 2]], excess: f64) -> Vec<[f64; 2]> {
  let mut path = vec![[-excess, -excess]];
  path.extend_from_slice(profile);
  path
}

fn mask2d_roundover(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let r = a
    .radius("r", "d", None)
    .or_else(|| a.num("cut").map(|c| c / (2f64.sqrt() - 1.0)))
    .unwrap_or(1.0);
  let excess = a.num_or("excess", 0.01);
  let inset = a.num_or("inset", 0.0);
  let segments = a.segments(r);
  let n = ((segments as f64 / 4.0).ceil() as u32).max(2);
  // A quarter circle tucked into the corner.
  let arc = arc_pts(n + 1, r, [r + inset, r + inset], 180.0, 90.0, true);
  let mut profile = vec![[-excess, r + inset]];
  profile.extend(arc.iter().copied());
  profile.push([r + inset, -excess]);
  let path = corner_mask_path(&profile, excess);
  finish_mask2d(lua, a, "mask2d_roundover", &path)
}

fn mask2d_cove(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let r = a.radius("r", "d", Some(1.0)).unwrap_or(1.0);
  let excess = a.num_or("excess", 0.01);
  let inset = a.num_or("inset", 0.0);
  let segments = a.segments(r);
  let n = ((segments as f64 / 4.0).ceil() as u32).max(2);
  // A cove bulges the other way: the circle is centred on the corner.
  let arc = arc_pts(n + 1, r, [inset, inset], 0.0, 90.0, true);
  let mut profile = vec![[-excess, r + inset]];
  profile.extend(arc.iter().rev().copied());
  profile.push([r + inset, -excess]);
  let path = corner_mask_path(&profile, excess);
  finish_mask2d(lua, a, "mask2d_cove", &path)
}

fn mask2d_chamfer(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let excess = a.num_or("excess", 0.01);
  let inset = a.num_or("inset", 0.0);
  // The cut may be given as the edge length, as the two legs, or as one leg
  // and the angle between them.
  let (x, y) = match (a.num("x"), a.num("y"), a.num("edge"), a.num("angle")) {
    (Some(x), Some(y), ..) => (x, y),
    (Some(x), None, _, Some(ang)) => (x, x * ang.to_radians().tan()),
    (None, Some(y), _, Some(ang)) => (y / ang.to_radians().tan(), y),
    (None, None, Some(e), Some(ang)) => {
      (e * ang.to_radians().cos(), e * ang.to_radians().sin())
    }
    (None, None, Some(e), None) => (e / 2f64.sqrt(), e / 2f64.sqrt()),
    _ => {
      let e = a.num_or("edge", 1.0);
      (e / 2f64.sqrt(), e / 2f64.sqrt())
    }
  };
  let profile = vec![
    [-excess, y + inset],
    [inset, y + inset],
    [x + inset, inset],
    [x + inset, -excess],
  ];
  let path = corner_mask_path(&profile, excess);
  finish_mask2d(lua, a, "mask2d_chamfer", &path)
}

fn mask2d_rabbet(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let size = match a.val("size") {
    Some(Val::Num(s)) => [s, s],
    Some(other) => match other.as_vec() {
      Some(v) if v.len() >= 2 => [v[0], v[1]],
      _ => [1.0, 1.0],
    },
    None => [1.0, 1.0],
  };
  let excess = a.num_or("excess", 0.01);
  // A rabbet is a plain rectangular step.
  let profile =
    vec![[-excess, size[1]], [size[0], size[1]], [size[0], -excess]];
  let path = corner_mask_path(&profile, excess);
  finish_mask2d(lua, a, "mask2d_rabbet", &path)
}

fn mask2d_dovetail(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let excess = a.num_or("excess", 0.01);
  let angle = a.num_or("angle", 30.0);
  let h = a.num("h").or_else(|| a.num("height")).unwrap_or(1.0);
  let w = a
    .num("w")
    .or_else(|| a.num("width"))
    .unwrap_or_else(|| h * angle.to_radians().tan());
  let shelf = a.num_or("shelf", 0.0);
  // The undercut leans back, so the joint cannot be pulled straight out.
  let profile = vec![
    [-excess, h],
    [w, h],
    [w - h * angle.to_radians().tan(), 0.0],
    [w - h * angle.to_radians().tan() + shelf, 0.0],
    [w - h * angle.to_radians().tan() + shelf, -excess],
  ];
  let path = corner_mask_path(&profile, excess);
  finish_mask2d(lua, a, "mask2d_dovetail", &path)
}

fn mask2d_teardrop(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let r = a.radius("r", "d", Some(1.0)).unwrap_or(1.0);
  let angle = a.num_or("angle", 45.0);
  let excess = a.num_or("excess", 0.01);
  let segments = a.segments(r);
  let n = ((segments as f64 / 4.0).ceil() as u32).max(2);
  // Rounded up to the overhang angle, then straight, so it prints without
  // support.
  let arc = arc_pts(n + 1, r, [r, r], 180.0, 90.0 - angle, true);
  let mut profile = vec![[-excess, r]];
  profile.extend(arc.iter().copied());
  // Below the overhang angle the profile runs straight down, which is what
  // lets it print without support.
  let last = *profile.last().unwrap_or(&[r, 0.0]);
  profile.push([last[0] + last[1] / angle.to_radians().tan(), -excess]);
  let path = corner_mask_path(&profile, excess);
  finish_mask2d(lua, a, "mask2d_teardrop", &path)
}

fn mask2d_ogee(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let Some(pattern) = a
    .val("pattern")
    .and_then(|v| v.as_list().map(|s| s.to_vec()))
  else {
    return a.err("pattern is required");
  };
  let excess = a.num_or("excess", 0.01);
  let segments = 32u32;

  // The pattern alternates a step name and its size, walking the profile
  // from the X leg round to the Y leg.
  let mut profile: Vec<[f64; 2]> = Vec::new();
  let mut at = [0.0f64, 0.0f64];
  let mut i = 0usize;
  while i + 1 < pattern.len() {
    let Some(name) = (match &pattern[i] {
      Val::Num(_) => None,
      _ => None,
    })
    .or_else(|| pattern.get(i).and(None::<String>)) else {
      // The names arrive as Lua strings, which the value type does not
      // carry, so the raw table is read instead.
      break;
    };
    let _ = name;
    i += 2;
  }
  // Read the pattern from the raw Lua table so the step names survive.
  if let Some(LuaValue::Table(t)) = a.raw("pattern") {
    let len = t.raw_len();
    let mut k = 1usize;
    while k < len {
      let name: String = t.get::<String>(k).unwrap_or_default();
      let size: f64 = t.get::<f64>(k + 1).unwrap_or(0.0);
      let n = ((segments as f64 / 4.0).ceil() as u32).max(2);
      match name.as_str() {
        "xstep" => at[0] += size,
        "ystep" => at[1] += size,
        "round" => {
          let arc =
            arc_pts(n + 1, size, [at[0], at[1] + size], 270.0, 90.0, true);
          profile.extend(arc.iter().copied());
          at = [at[0] + size, at[1] + size];
        }
        "fillet" => {
          let arc =
            arc_pts(n + 1, size, [at[0] + size, at[1]], 180.0, -90.0, true);
          profile.extend(arc.iter().copied());
          at = [at[0] + size, at[1] + size];
        }
        _ => {}
      }
      profile.push(at);
      k += 2;
    }
  }
  if profile.is_empty() {
    return a.err("the pattern produced no profile");
  }
  let mut path = vec![[-excess, at[1]]];
  path.extend(profile.iter().rev().copied());
  path.push([-excess, -excess]);
  finish_mask2d(lua, a, "mask2d_ogee", &path)
}

fn finish_mask2d(
  lua: &Lua,
  a: &Args,
  function: &'static str,
  path: &[[f64; 2]],
) -> LuaResult<LuaValue> {
  let attachable = Attachable::new(Geom::RegionExtent {
    points: path.to_vec(),
  });
  let node = reorient(crate::bosl::shapes2d::path_node(path), a, &attachable)?;
  as_sketch(lua, function, node)
}

// ---------------------------------------------------------------------------
// 3D masks
// ---------------------------------------------------------------------------

/// Extrude a 2D mask profile along an edge.
fn edge_mask_from(
  lua: &Lua,
  a: &Args,
  function: &'static str,
  profile: &[[f64; 2]],
  length: f64,
) -> LuaResult<LuaValue> {
  let node = ScadNode::LinearExtrude {
    height: length as f32,
    center: true,
    twist: 0.0,
    slices: 1,
    scale: 1.0,
    child: Box::new(crate::bosl::shapes2d::path_node(profile)),
  };
  let attachable = Attachable::new(Geom::Prismoid {
    size: [1.0, 1.0, length],
    size2: [1.0, 1.0],
    shift: [0.0, 0.0],
    axis: [0.0, 0.0, 1.0],
  });
  as_geometry(lua, function, reorient(node, a, &attachable)?)
}

fn length_of(a: &Args) -> f64 {
  a.num("l")
    .or_else(|| a.num("h"))
    .or_else(|| a.num("length"))
    .or_else(|| a.num("height"))
    .unwrap_or(1.0)
}

/// The corner profile a rounding mask cuts, in the plane across the edge.
///
/// A 3D edge mask sits *on* the edge it works on, with the material it
/// removes reaching inward — so the profile lies in the third quadrant and
/// the fillet's centre is at `(-r, -r)`. Placing the mask at a box's edge
/// then rounds that edge, with no further orienting.
fn rounding_profile(r: f64, segments: u32, excess: f64) -> Vec<[f64; 2]> {
  let n = ((segments as f64 / 4.0).ceil() as u32).max(2);
  let arc = arc_pts(n + 1, r, [r, r], 180.0, 90.0, true);
  let mut path = vec![[-excess, -excess], [-excess, r]];
  path.extend(arc.iter().copied());
  path.push([r, -excess]);
  inward(&path)
}

/// Turn a corner profile so its material reaches inward from the edge.
fn inward(path: &[[f64; 2]]) -> Vec<[f64; 2]> {
  path.iter().map(|p| [-p[0], -p[1]]).collect()
}

fn rounding_edge_mask(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let r = a.radius("r", "d", Some(1.0)).unwrap_or(1.0);
  let excess = a.num_or("excess", 0.01);
  let profile = rounding_profile(r, a.segments(r), excess);
  edge_mask_from(lua, a, "rounding_edge_mask", &profile, length_of(a))
}

fn chamfer_edge_mask(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let c = a.num_or("chamfer", 1.0);
  let excess = a.num_or("excess", 0.1);
  let profile = inward(&[[-excess, -excess], [-excess, c], [c, -excess]]);
  edge_mask_from(lua, a, "chamfer_edge_mask", &profile, length_of(a))
}

fn teardrop_edge_mask(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let r = a.radius("r", "d", Some(1.0)).unwrap_or(1.0);
  let angle = a.num_or("angle", 45.0);
  let excess = a.num_or("excess", 0.1);
  let n = ((a.segments(r) as f64 / 4.0).ceil() as u32).max(2);
  let arc = arc_pts(n + 1, r, [r, r], 180.0, 90.0 - angle, true);
  let mut profile = vec![[-excess, -excess], [-excess, r]];
  profile.extend(arc.iter().copied());
  let last = *profile.last().unwrap_or(&[r, 0.0]);
  profile.push([last[0] + last[1] / angle.to_radians().tan(), -excess]);
  let profile = inward(&profile);
  edge_mask_from(lua, a, "teardrop_edge_mask", &profile, length_of(a))
}

/// The solid to take out of a corner where three masked edges meet.
fn corner_mask_solid(r: f64, segments: u32, sphere: bool) -> ScadNode {
  let cube = ScadNode::Translate {
    x: (-r / 2.0) as f32,
    y: (-r / 2.0) as f32,
    z: (-r / 2.0) as f32,
    child: Box::new(ScadNode::Cube {
      w: r as f32,
      d: r as f32,
      h: r as f32,
      center: true,
    }),
  };
  let keep = if sphere {
    ScadNode::Translate {
      x: -r as f32,
      y: -r as f32,
      z: -r as f32,
      child: Box::new(ScadNode::Sphere {
        r: r as f32,
        segments,
      }),
    }
  } else {
    // A chamfered corner is cut by the plane through the three edge cuts, so
    // the part kept is the half-space beyond it. A big box is turned so one
    // face lies on that plane and pushed back until it starts there.
    let n = -1.0 / 3f64.sqrt();
    let side = r * 8.0;
    let reach = r * n.abs() + side / 2.0;
    crate::bosl::attach::transform(
      ScadNode::Cube {
        w: side as f32,
        d: side as f32,
        h: side as f32,
        center: true,
      },
      Mat4::translate([n * reach, n * reach, n * reach])
        .mul(&Mat4::rot_from_to([0.0, 0.0, 1.0], [n, n, n])),
    )
  };
  ScadNode::Difference(vec![cube, keep])
}

fn rounding_corner_mask(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let r = a.radius("r", "d", Some(1.0)).unwrap_or(1.0);
  let node = corner_mask_solid(r, a.segments(r), true);
  let attachable = Attachable::new(Geom::Prismoid {
    size: [r, r, r],
    size2: [r, r],
    shift: [0.0, 0.0],
    axis: [0.0, 0.0, 1.0],
  });
  as_geometry(lua, "rounding_corner_mask", reorient(node, a, &attachable)?)
}

fn chamfer_corner_mask(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let c = a.num_or("chamfer", 1.0);
  let node = corner_mask_solid(c, a.segments(c), false);
  let attachable = Attachable::new(Geom::Prismoid {
    size: [c, c, c],
    size2: [c, c],
    shift: [0.0, 0.0],
    axis: [0.0, 0.0, 1.0],
  });
  as_geometry(lua, "chamfer_corner_mask", reorient(node, a, &attachable)?)
}

fn teardrop_corner_mask(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  rounding_corner_mask(lua, a)
}

/// Revolve a corner profile about the Z axis, which is how the cylinder
/// masks are built.
fn revolved_mask(
  lua: &Lua,
  a: &Args,
  function: &'static str,
  profile: &[[f64; 2]],
  facets: u32,
) -> LuaResult<LuaValue> {
  let vnf = Vnf::rotate_sweep(profile, 360.0, facets, true);
  let attachable = Attachable::new(Geom::Conoid {
    r1: [1.0, 1.0],
    r2: [1.0, 1.0],
    l: 1.0,
    shift: [0.0, 0.0],
    axis: [0.0, 0.0, 1.0],
  });
  as_geometry(lua, function, reorient(vnf.to_node(), a, &attachable)?)
}

fn rounding_cylinder_mask(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let r = a.radius("r", "d", Some(1.0)).unwrap_or(1.0);
  let rounding = a.num_or("rounding", 0.5);
  let n = ((a.segments(rounding) as f64 / 4.0).ceil() as u32).max(2);
  // The ring that sits outside the rounded top rim of a cylinder.
  let arc = arc_pts(n + 1, rounding, [r - rounding, rounding], 0.0, 90.0, true);
  let mut profile = vec![
    [r + 0.01, -0.01],
    [r + 0.01, rounding + 0.01],
    [r - rounding, rounding + 0.01],
  ];
  profile.extend(arc.iter().rev().copied());
  profile.push([r, -0.01]);
  revolved_mask(
    lua,
    a,
    "rounding_cylinder_mask",
    &crate::bosl::vnf::ccw(profile),
    a.segments(r),
  )
}

fn rounding_hole_mask(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let r = a.radius("r", "d", Some(1.0)).unwrap_or(1.0);
  let rounding = a.num_or("rounding", 0.5);
  let excess = a.num_or("excess", 0.1);
  let n = ((a.segments(rounding) as f64 / 4.0).ceil() as u32).max(2);
  // The ring that flares the mouth of a hole outward.
  let arc = arc_pts(n + 1, rounding, [r + rounding, 0.0], 180.0, 90.0, true);
  let mut profile = vec![[r - excess, -rounding - excess], [r - excess, 0.0]];
  profile.extend(arc.iter().copied());
  profile.push([r + rounding, -rounding - excess]);
  revolved_mask(
    lua,
    a,
    "rounding_hole_mask",
    &crate::bosl::vnf::ccw(profile),
    a.segments(r),
  )
}

fn chamfer_cylinder_mask(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let r = a.radius("r", "d", Some(1.0)).unwrap_or(1.0);
  let c = a.num_or("chamfer", 0.5);
  let ang = a.num_or("ang", 45.0);
  let reach = c / ang.to_radians().tan();
  let profile = vec![
    [r - c, 0.01],
    [r + 0.01, 0.01],
    [r + 0.01, -reach - 0.01],
    [r, -reach],
  ];
  revolved_mask(
    lua,
    a,
    "chamfer_cylinder_mask",
    &crate::bosl::vnf::ccw(profile),
    a.segments(r),
  )
}

/// Apply a 2D profile along an edge or round a face of a shape.
///
/// LuaCAD has no attachment parent to read the edge from, so these take the
/// shape and the edge selection directly.
fn profile_stub(
  function: &'static str,
) -> impl Fn(&Lua, &Args) -> LuaResult<LuaValue> {
  move |_lua, a| {
    a.err(format!(
      "{function}() attaches a profile to a parent shape's edges, which \
       LuaCAD has no equivalent for. Use bosl.cuboid {{ …, rounding = … }} \
       or subtract bosl.rounding_edge_mask() yourself."
    ))
  }
}

// ---------------------------------------------------------------------------
// Registration
// ---------------------------------------------------------------------------

fn register_one(
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
  // --- partitions ---
  register_one(
    lua,
    bosl,
    "half_of",
    &["v", "cp", "s", "planar", "p"],
    half_of,
  )?;
  const HALF_X: &[&str] = &["s", "x", "planar", "p"];
  const HALF_Y: &[&str] = &["s", "y", "planar", "p"];
  const HALF_Z: &[&str] = &["s", "z", "planar", "p"];
  for (name, dir, param, params) in [
    ("left_half", [-1.0, 0.0, 0.0], "x", HALF_X),
    ("right_half", [1.0, 0.0, 0.0], "x", HALF_X),
    ("front_half", [0.0, -1.0, 0.0], "y", HALF_Y),
    ("back_half", [0.0, 1.0, 0.0], "y", HALF_Y),
    ("bottom_half", [0.0, 0.0, -1.0], "z", HALF_Z),
    ("top_half", [0.0, 0.0, 1.0], "z", HALF_Z),
  ] {
    register_one(lua, bosl, name, params, axis_half(name, dir, param))?;
  }
  register_one(
    lua,
    bosl,
    "partition_mask",
    &["l", "w", "h", "cutsize", "cutpath", "gap", "inverse"],
    partition_mask,
  )?;
  register_one(
    lua,
    bosl,
    "partition_cut_mask",
    &["l", "w", "h", "cutsize", "cutpath", "gap"],
    partition_cut_mask,
  )?;
  register_one(
    lua,
    bosl,
    "partition",
    &["size", "spread", "cutsize", "cutpath", "gap", "spin", "p"],
    partition,
  )?;

  // --- 2D masks ---
  register_one(
    lua,
    bosl,
    "mask2d_roundover",
    &[
      "r",
      "inset",
      "mask_angle",
      "excess",
      "flat_top",
      "d",
      "h",
      "height",
      "cut",
      "quarter_round",
      "joint",
    ],
    mask2d_roundover,
  )?;
  register_one(
    lua,
    bosl,
    "mask2d_cove",
    &[
      "r",
      "inset",
      "mask_angle",
      "excess",
      "flat_top",
      "bulge",
      "d",
      "h",
      "height",
      "quarter_round",
    ],
    mask2d_cove,
  )?;
  register_one(
    lua,
    bosl,
    "mask2d_chamfer",
    &[
      "edge",
      "angle",
      "inset",
      "excess",
      "mask_angle",
      "flat_top",
      "x",
      "y",
      "h",
      "w",
      "height",
      "width",
    ],
    mask2d_chamfer,
  )?;
  register_one(
    lua,
    bosl,
    "mask2d_rabbet",
    &["size", "mask_angle", "excess"],
    mask2d_rabbet,
  )?;
  register_one(
    lua,
    bosl,
    "mask2d_dovetail",
    &[
      "edge",
      "angle",
      "shelf",
      "inset",
      "mask_angle",
      "excess",
      "flat_top",
      "w",
      "h",
      "width",
      "height",
      "slope",
      "x",
      "y",
    ],
    mask2d_dovetail,
  )?;
  register_one(
    lua,
    bosl,
    "mask2d_teardrop",
    &[
      "r",
      "angle",
      "inset",
      "mask_angle",
      "excess",
      "flat_top",
      "d",
      "h",
      "height",
      "cut",
      "joint",
    ],
    mask2d_teardrop,
  )?;
  register_one(
    lua,
    bosl,
    "mask2d_ogee",
    &["pattern", "excess"],
    mask2d_ogee,
  )?;
  // BOSL2 names the smooth roundover this way too.
  register_one(
    lua,
    bosl,
    "mask2d_smooth",
    &["r", "inset", "mask_angle", "excess", "d", "cut", "joint"],
    mask2d_roundover,
  )?;

  // --- 3D masks ---
  register_one(
    lua,
    bosl,
    "rounding_edge_mask",
    &[
      "l", "r", "ang", "r1", "r2", "excess", "d1", "d2", "d", "length", "h",
      "height",
    ],
    rounding_edge_mask,
  )?;
  register_one(
    lua,
    bosl,
    "chamfer_edge_mask",
    &["l", "chamfer", "excess", "h", "length", "height"],
    chamfer_edge_mask,
  )?;
  register_one(
    lua,
    bosl,
    "teardrop_edge_mask",
    &["l", "r", "angle", "excess", "d", "h", "height", "length"],
    teardrop_edge_mask,
  )?;
  register_one(
    lua,
    bosl,
    "rounding_corner_mask",
    &["r", "ang", "d", "style", "excess"],
    rounding_corner_mask,
  )?;
  register_one(
    lua,
    bosl,
    "chamfer_corner_mask",
    &["chamfer"],
    chamfer_corner_mask,
  )?;
  register_one(
    lua,
    bosl,
    "teardrop_corner_mask",
    &["r", "angle", "excess", "d"],
    teardrop_corner_mask,
  )?;
  register_one(
    lua,
    bosl,
    "rounding_cylinder_mask",
    &["r", "rounding", "d"],
    rounding_cylinder_mask,
  )?;
  register_one(
    lua,
    bosl,
    "rounding_hole_mask",
    &["r", "rounding", "excess", "d"],
    rounding_hole_mask,
  )?;
  register_one(
    lua,
    bosl,
    "chamfer_cylinder_mask",
    &["r", "chamfer", "d", "ang", "from_end"],
    chamfer_cylinder_mask,
  )?;

  // The attachment-driven profile helpers have no standalone meaning.
  for name in [
    "edge_mask",
    "corner_mask",
    "face_mask",
    "edge_profile",
    "edge_profile_asym",
    "corner_profile",
    "face_profile",
    "polygon_edge_mask",
  ] {
    register_one(
      lua,
      bosl,
      name,
      &["edges", "except", "excess"],
      profile_stub(name),
    )?;
  }
  Ok(())
}

#[cfg(test)]
mod tests {
  use crate::bosl::register_bosl;
  use mlua::Lua;

  fn volume(code: &str) -> f64 {
    let geoms = crate::lua_engine::execute_lua(code).unwrap();
    let node = geoms[0].scad.clone().unwrap();
    crate::export::materialize_scad_manifold(&node).volume()
  }

  fn bbox(code: &str) -> ([f32; 3], [f32; 3]) {
    let geoms = crate::lua_engine::execute_lua(code).unwrap();
    let node = geoms[0].scad.clone().unwrap();
    crate::export::materialize_scad_manifold(&node).bounding_box()
  }

  #[test]
  fn halving_a_cube_leaves_half_of_it() {
    let v = volume(
      "render(bosl.top_half({p = cube { {10, 10, 10}, center = true }}))",
    );
    assert!((v - 500.0).abs() < 1e-3, "{v}");
    let (lo, _) =
      bbox("render(bosl.top_half({p = cube { {10, 10, 10}, center = true }}))");
    assert!(lo[2].abs() < 1e-3, "{lo:?}");
  }

  #[test]
  fn each_half_keeps_the_side_its_name_says() {
    let (lo, hi) = bbox(
      "render(bosl.right_half({p = cube { {10, 10, 10}, center = true }}))",
    );
    assert!(lo[0].abs() < 1e-3, "{lo:?}");
    assert!((hi[0] - 5.0).abs() < 1e-3, "{hi:?}");
    let (lo, hi) = bbox(
      "render(bosl.left_half({p = cube { {10, 10, 10}, center = true }}))",
    );
    assert!((lo[0] + 5.0).abs() < 1e-3, "{lo:?}");
    assert!(hi[0].abs() < 1e-3, "{hi:?}");
  }

  #[test]
  fn an_offset_cut_moves_the_plane() {
    let v = volume(
      "render(bosl.top_half({z = 2, p = cube { {10, 10, 10}, center = true }}))",
    );
    assert!((v - 300.0).abs() < 1e-3, "{v}");
  }

  #[test]
  fn half_of_cuts_along_an_arbitrary_plane() {
    let v = volume(
      "render(bosl.half_of({v = {0,0,1}, p = cube { {10, 10, 10}, center = true }}))",
    );
    assert!((v - 500.0).abs() < 1e-3, "{v}");
  }

  #[test]
  fn a_rounding_edge_mask_has_the_volume_it_removes() {
    let v = volume(
      "render(bosl.rounding_edge_mask({l = 10, r = 3, excess = 0, fn = 128}))",
    );
    // The corner square minus its quarter circle, over the length.
    let ideal = (9.0 - std::f64::consts::PI * 9.0 / 4.0) * 10.0;
    assert!((v - ideal).abs() / ideal < 0.02, "{v} vs {ideal}");
  }

  #[test]
  fn a_chamfer_edge_mask_is_a_triangular_prism() {
    let v = volume(
      "render(bosl.chamfer_edge_mask({l = 10, chamfer = 3, excess = 0}))",
    );
    let ideal = 3.0 * 3.0 / 2.0 * 10.0;
    assert!((v - ideal).abs() / ideal < 0.05, "{v} vs {ideal}");
  }

  #[test]
  fn a_rounding_corner_mask_leaves_a_sphere_octant_behind() {
    let v = volume("render(bosl.rounding_corner_mask({r = 4, fn = 128}))");
    let ideal = 64.0 - std::f64::consts::PI * 64.0 / 6.0;
    assert!((v - ideal).abs() / ideal < 0.02, "{v} vs {ideal}");
  }

  #[test]
  fn a_chamfer_corner_mask_is_the_tetrahedron_it_cuts_off() {
    let v = volume("render(bosl.chamfer_corner_mask(4))");
    // The corner cut away is the tetrahedron between the three edge cuts.
    let ideal = 64.0 / 6.0;
    assert!((v - ideal).abs() / ideal < 0.02, "{v} vs {ideal}");
  }

  #[test]
  fn subtracting_an_edge_mask_rounds_the_edge() {
    let plain = volume("render(cube(20, true))");
    let rounded = volume(
      "render(cube { {20, 20, 20}, center = true }
              - bosl.rounding_edge_mask({l = 30, r = 3, excess = 0.1,
                                         fn = 128})
                :translate(10, 10, 0))",
    );
    let lost = (9.0 - std::f64::consts::PI * 9.0 / 4.0) * 20.0;
    assert!(
      (plain - rounded - lost).abs() / lost < 0.05,
      "{}",
      plain - rounded
    );
  }

  #[test]
  fn a_2d_mask_is_a_sketch_that_can_be_extruded() {
    let v = volume(
      "render((bosl.mask2d_roundover({r = 3, excess = 0, fn = 128}))
              :linear_extrude(10))",
    );
    let ideal = (9.0 - std::f64::consts::PI * 9.0 / 4.0) * 10.0;
    assert!((v - ideal).abs() / ideal < 0.02, "{v} vs {ideal}");
  }

  #[test]
  fn a_2d_chamfer_mask_cuts_a_triangle() {
    let v =
      volume("render((bosl.mask2d_chamfer({edge = 4})):linear_extrude(10))");
    // A 4-long edge at 45 degrees has legs of 4/sqrt(2).
    let leg = 4.0 / 2f64.sqrt();
    let ideal = leg * leg / 2.0 * 10.0;
    assert!((v - ideal).abs() / ideal < 0.1, "{v} vs {ideal}");
  }

  #[test]
  fn a_rabbet_mask_is_a_rectangular_step() {
    let v = volume("render((bosl.mask2d_rabbet({4, 2})):linear_extrude(10))");
    assert!((v - 4.0 * 2.0 * 10.0).abs() / 80.0 < 0.1, "{v}");
  }

  #[test]
  fn a_partition_mask_is_a_solid_with_a_toothed_face() {
    let v = volume("render(bosl.partition_mask({l = 50, w = 20, h = 30}))");
    assert!(v > 0.0, "{v}");
  }

  #[test]
  fn the_attachment_only_helpers_explain_themselves() {
    let lua = Lua::new();
    register_bosl(&lua).unwrap();
    let err = lua
      .load("return bosl.edge_profile()")
      .eval::<mlua::Value>()
      .unwrap_err()
      .to_string();
    assert!(err.contains("rounding_edge_mask"), "{err}");
  }
}
