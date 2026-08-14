//! BOSL2's `skin.scad`, `rounding.scad` and `drawing.scad`.
//!
//! These are the sweep operations: lofting between cross-sections, running a
//! profile along a path, and rounding a polyline's corners. They share the
//! machinery for lining two outlines up and for building a frame that
//! follows a curve.

use mlua::{Lua, Result as LuaResult, Value as LuaValue};

use crate::bosl::value::{Args, Val, v3};
use crate::bosl::vecmath::Mat4;
use crate::bosl::vnf::{Caps, Vnf, arc_pts};
use crate::geometry::{CsgGeometry, CsgSketch};
use crate::scad_export::ScadNode;

const EPS: f64 = 1e-9;

fn sub(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
  [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn add(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
  [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

fn scale(a: [f64; 3], k: f64) -> [f64; 3] {
  [a[0] * k, a[1] * k, a[2] * k]
}

fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
  a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
  [
    a[1] * b[2] - a[2] * b[1],
    a[2] * b[0] - a[0] * b[2],
    a[0] * b[1] - a[1] * b[0],
  ]
}

fn norm(a: [f64; 3]) -> f64 {
  dot(a, a).sqrt()
}

fn unit(a: [f64; 3]) -> [f64; 3] {
  let n = norm(a);
  if n < EPS { [0.0; 3] } else { scale(a, 1.0 / n) }
}

/// Wrap a built solid as a BOSL2 call, so `.scad` export still writes it.
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

/// Read the outline a sweep is built from, whether given as a list of points
/// or as a LuaCAD sketch.
fn read_profile(a: &Args, name: &str) -> LuaResult<Vec<[f64; 2]>> {
  if let Some(LuaValue::UserData(ud)) = a.raw(name)
    && let Ok(sketch) = ud.borrow::<CsgSketch>()
  {
    if let Some(path) = outline_of(sketch.scad.as_ref()) {
      return Ok(path);
    }
    return a.err(format!("{name} is a sketch with no outline to sweep"));
  }
  match a.val(name).and_then(|v| v.as_matrix()) {
    Some(m) if m.len() >= 3 => Ok(
      m.iter()
        .map(|p| [p[0], *p.get(1).unwrap_or(&0.0)])
        .collect(),
    ),
    _ => a.err(format!(
      "{name} must be an outline of at least three points"
    )),
  }
}

/// Pull a polygon outline back out of a sketch's node tree.
fn outline_of(node: Option<&ScadNode>) -> Option<Vec<[f64; 2]>> {
  match node? {
    ScadNode::Polygon { points } => {
      Some(points.iter().map(|p| [p[0] as f64, p[1] as f64]).collect())
    }
    ScadNode::BoslCall {
      native: Some(n), ..
    } => outline_of(Some(n)),
    ScadNode::Translate { child, .. }
    | ScadNode::Rotate { child, .. }
    | ScadNode::Color { child, .. } => outline_of(Some(child)),
    _ => None,
  }
}

/// The cross-sections a loft runs through, all resampled to one point count.
fn read_profiles(a: &Args) -> LuaResult<Vec<Vec<[f64; 3]>>> {
  let Some(items) = a
    .val("profiles")
    .and_then(|v| v.as_list().map(|s| s.to_vec()))
  else {
    return a.err("profiles must be a list of outlines");
  };
  if items.len() < 2 {
    return a.err("at least two profiles are needed");
  }
  let mut out = Vec::with_capacity(items.len());
  for p in &items {
    match p.as_matrix() {
      Some(m) if m.len() >= 3 => out.push(m.iter().map(|q| v3(q)).collect()),
      _ => return a.err("every profile needs at least three points"),
    }
  }
  Ok(out)
}

/// Resample a closed outline to `n` evenly spaced points.
fn resample_loop(path: &[[f64; 3]], n: usize) -> Vec<[f64; 3]> {
  let mut cum = vec![0.0];
  for i in 0..path.len() {
    let d = norm(sub(path[(i + 1) % path.len()], path[i]));
    cum.push(cum[i] + d);
  }
  let total = cum[path.len()];
  if total < EPS {
    return vec![path[0]; n];
  }
  (0..n)
    .map(|k| {
      let d = total * k as f64 / n as f64;
      let i = cum
        .iter()
        .rposition(|c| *c <= d + 1e-12)
        .unwrap_or(0)
        .min(path.len() - 1);
      let seg = cum[i + 1] - cum[i];
      let t = if seg < 1e-12 { 0.0 } else { (d - cum[i]) / seg };
      let p = path[i];
      let q = path[(i + 1) % path.len()];
      add(p, scale(sub(q, p), t))
    })
    .collect()
}

/// Turn the outline so its first point lies nearest the reference's, which
/// is what keeps a loft from twisting between two profiles.
fn align_loop(reference: &[[f64; 3]], loop_: &[[f64; 3]]) -> Vec<[f64; 3]> {
  let n = loop_.len();
  let cost = |shift: usize| -> f64 {
    (0..n)
      .map(|i| {
        let d = sub(
          loop_[(i + shift) % n],
          reference[i.min(reference.len() - 1)],
        );
        dot(d, d)
      })
      .sum()
  };
  let best = (0..n)
    .min_by(|x, y| cost(*x).total_cmp(&cost(*y)))
    .unwrap_or(0);
  (0..n).map(|i| loop_[(i + best) % n]).collect()
}

// ---------------------------------------------------------------------------
// skin
// ---------------------------------------------------------------------------

fn skin(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let profiles = read_profiles(a)?;
  let slices = a.int("slices").unwrap_or(0).max(0) as usize;
  let closed = a.bool_or("closed", false);
  let caps = a.bool("caps").unwrap_or(!closed);

  // Every cross-section has to have the same number of points before they
  // can be joined up, so they are all resampled to the largest count.
  let n = profiles.iter().map(|p| p.len()).max().unwrap_or(3);
  let mut sections: Vec<Vec<[f64; 3]>> = Vec::new();
  for (i, p) in profiles.iter().enumerate() {
    let resampled = resample_loop(p, n);
    sections.push(match sections.last() {
      Some(prev) => align_loop(prev, &resampled),
      None => {
        let _ = i;
        resampled
      }
    });
  }

  // Extra slices between each pair make the surface between them smoother.
  let mut rows: Vec<Vec<[f64; 3]>> = Vec::new();
  let pairs = if closed {
    sections.len()
  } else {
    sections.len() - 1
  };
  for i in 0..pairs {
    let from = &sections[i];
    let to = &sections[(i + 1) % sections.len()];
    for s in 0..=slices {
      let t = s as f64 / (slices + 1) as f64;
      rows.push(
        from
          .iter()
          .zip(to.iter())
          .map(|(p, q)| add(scale(*p, 1.0 - t), scale(*q, t)))
          .collect(),
      );
    }
  }
  if !closed {
    rows.push(sections[sections.len() - 1].clone());
  }

  let vnf = Vnf::vertex_array(
    &rows,
    if caps && !closed {
      Caps::BOTH
    } else {
      Caps::NONE
    },
    true,
    closed,
  );
  as_geometry(lua, "skin", vnf.to_node())
}

// ---------------------------------------------------------------------------
// sweeping a profile along a path
// ---------------------------------------------------------------------------

/// A frame at each point of a path, carried along it without twisting.
///
/// Each frame is the previous one turned by the smallest rotation that lines
/// its axis up with the new tangent. Building each frame from scratch would
/// let it spin about the tangent wherever the path passes vertical.
fn parallel_frames(path: &[[f64; 3]], closed: bool) -> Vec<Mat4> {
  let n = path.len();
  if n == 0 {
    return vec![];
  }
  let tangent = |i: usize| -> [f64; 3] {
    let prev = if i == 0 {
      if closed { path[n - 1] } else { path[0] }
    } else {
      path[i - 1]
    };
    let next = if i == n - 1 {
      if closed { path[0] } else { path[n - 1] }
    } else {
      path[i + 1]
    };
    unit(sub(next, prev))
  };

  let mut frames = Vec::with_capacity(n);
  let mut up = {
    let t = tangent(0);
    // Any direction across the first tangent will do to start.
    let seed = if t[2].abs() < 0.9 {
      [0.0, 0.0, 1.0]
    } else {
      [1.0, 0.0, 0.0]
    };
    unit(cross(cross(t, seed), t))
  };
  for (i, point) in path.iter().enumerate() {
    let t = tangent(i);
    if i > 0 {
      // Carry the reference direction across by the same turn the tangent
      // made, so the frame never spins about the path.
      let prev_t = tangent(i - 1);
      let axis = cross(prev_t, t);
      if norm(axis) > EPS {
        let ang = dot(prev_t, t).clamp(-1.0, 1.0).acos().to_degrees();
        up = Mat4::rot_by_axis(axis, ang).apply(up);
      }
    }
    let x = unit(cross(up, t));
    let y = unit(cross(t, x));
    frames.push(Mat4([
      x[0], y[0], t[0], point[0], //
      x[1], y[1], t[1], point[1], //
      x[2], y[2], t[2], point[2], //
      0.0, 0.0, 0.0, 1.0,
    ]));
  }
  frames
}

fn path_sweep(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let shape = read_profile(a, "shape")?;
  let path = crate::bosl::paths::read_path(a, "path")?;
  if path.len() < 2 {
    return a.err("the path needs at least two points");
  }
  let closed = a.bool_or("closed", false);
  let twist = a.num_or("twist", 0.0);
  let scale_end = a.num_or("scale", 1.0);
  let caps = a.bool("caps").unwrap_or(!closed);

  let frames = parallel_frames(&path, closed);
  let n = frames.len();
  let rows: Vec<Vec<[f64; 3]>> = frames
    .iter()
    .enumerate()
    .map(|(i, m)| {
      let t = if n > 1 {
        i as f64 / (n - 1) as f64
      } else {
        0.0
      };
      // Twist and taper are spread evenly along the path.
      let spin = Mat4::zrot(twist * t);
      let k = 1.0 + (scale_end - 1.0) * t;
      shape
        .iter()
        .map(|p| m.apply(spin.apply([p[0] * k, p[1] * k, 0.0])))
        .collect()
    })
    .collect();

  let vnf = Vnf::vertex_array(
    &rows,
    if caps && !closed {
      Caps::BOTH
    } else {
      Caps::NONE
    },
    true,
    closed,
  );
  as_geometry(lua, "path_sweep", vnf.to_node())
}

fn path_sweep2d(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  path_sweep(lua, a)
}

/// Sweep a profile through a stack of transformations.
fn sweep(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let shape = read_profile(a, "shape")?;
  let Some(mats) = a
    .val("transforms")
    .and_then(|v| v.as_list().map(|s| s.to_vec()))
  else {
    return a.err("transforms must be a list of 4x4 matrices");
  };
  let closed = a.bool_or("closed", false);
  let caps = a.bool("caps").unwrap_or(!closed);

  let mut rows: Vec<Vec<[f64; 3]>> = Vec::with_capacity(mats.len());
  for t in &mats {
    let Some(m) = t.as_matrix() else {
      return a.err("every transform must be a 4x4 matrix");
    };
    let mut mat = Mat4::identity();
    for (r, row) in m.iter().take(4).enumerate() {
      for (c, v) in row.iter().take(4).enumerate() {
        mat.0[r * 4 + c] = *v;
      }
    }
    rows.push(shape.iter().map(|p| mat.apply([p[0], p[1], 0.0])).collect());
  }
  if rows.len() < 2 {
    return a.err("at least two transforms are needed");
  }

  let vnf = Vnf::vertex_array(
    &rows,
    if caps && !closed {
      Caps::BOTH
    } else {
      Caps::NONE
    },
    true,
    closed,
  );
  as_geometry(lua, "sweep", vnf.to_node())
}

/// Extrude a 2D region, optionally twisting and tapering as it goes.
fn linear_sweep(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let region = read_profile(a, "region")?;
  let height = a
    .num("height")
    .or_else(|| a.num("h"))
    .or_else(|| a.num("l"))
    .unwrap_or(1.0);
  let twist = a.num_or("twist", 0.0);
  let taper = a.num_or("scale", 1.0);
  let shift = a.vec2("shift").unwrap_or([0.0, 0.0]);
  let slices = a
    .int("slices")
    .map(|s| s.max(1) as usize)
    .unwrap_or_else(|| ((twist.abs() / 5.0).ceil() as usize).max(1));

  let rows: Vec<Vec<[f64; 3]>> = (0..=slices)
    .map(|i| {
      let t = i as f64 / slices as f64;
      let z = height * t;
      let k = 1.0 + (taper - 1.0) * t;
      let spin = Mat4::zrot(twist * t);
      region
        .iter()
        .map(|p| {
          let q = spin.apply([p[0] * k, p[1] * k, 0.0]);
          [q[0] + shift[0] * t, q[1] + shift[1] * t, z]
        })
        .collect()
    })
    .collect();

  let vnf = Vnf::vertex_array(&rows, Caps::BOTH, true, false);
  let node = vnf.to_node();
  // `center` straddles the XY plane instead of standing on it.
  let node = if a.bool_or("center", false) {
    crate::bosl::attach::transform(
      node,
      Mat4::translate([0.0, 0.0, -height / 2.0]),
    )
  } else {
    node
  };
  as_geometry(lua, "linear_sweep", node)
}

/// Sweep a profile round a helix, which is how threads are cut.
fn spiral_sweep(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let poly = read_profile(a, "poly")?;
  let h = a
    .num("h")
    .or_else(|| a.num("height"))
    .or_else(|| a.num("l"))
    .or_else(|| a.num("length"))
    .unwrap_or(1.0);
  let turns = a.num_or("turns", 1.0);
  let r1 = a
    .radius_end("r1", "d1", "r", "d", Some(50.0))
    .unwrap_or(50.0);
  let r2 = a.radius_end("r2", "d2", "r", "d", Some(r1)).unwrap_or(r1);
  let internal = a.bool_or("internal", false);
  if turns.abs() < EPS {
    return a.err("turns cannot be zero");
  }

  let steps = ((turns.abs() * 72.0).ceil() as usize).max(8);
  let rows: Vec<Vec<[f64; 3]>> = (0..=steps)
    .map(|i| {
      let t = i as f64 / steps as f64;
      let ang = 360.0 * turns * t;
      let r = r1 + (r2 - r1) * t;
      let z = h * (t - 0.5);
      let (s, c) = ang.to_radians().sin_cos();
      // The profile lies in the plane containing the axis, so its x runs
      // outward and its y runs along the axis.
      poly
        .iter()
        .map(|p| {
          let radial = if internal { r - p[0] } else { r + p[0] };
          [radial * c, radial * s, z + p[1]]
        })
        .collect()
    })
    .collect();

  let vnf = Vnf::vertex_array(&rows, Caps::BOTH, true, false);
  as_geometry(lua, "spiral_sweep", vnf.to_node())
}

// ---------------------------------------------------------------------------
// rounding
// ---------------------------------------------------------------------------

/// Round or chamfer the corners of a polyline.
fn round_corners(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let path = crate::bosl::paths::read_path(a, "path")?;
  let dim = a
    .val("path")
    .and_then(|v| v.as_matrix())
    .and_then(|m| m.first().map(|p| p.len()))
    .unwrap_or(3)
    .clamp(2, 3);
  let closed = a.bool_or("closed", true);
  let method = a.string("method").unwrap_or_else(|| "circle".to_string());

  // The size can be given as the radius, the depth of the cut, or the length
  // taken off each leg; each implies the others at the corner's angle.
  let radius = a.num("radius").or_else(|| a.num("r"));
  let cut = a.num("cut");
  let joint = a.num("joint").or_else(|| a.num("width"));
  if radius.is_none() && cut.is_none() && joint.is_none() {
    return a.err("give one of radius, r, cut, joint or width");
  }

  let n = path.len();
  if n < 3 {
    return Val::list(path.iter().map(|p| Val::vec(p[..dim].to_vec())))
      .to_lua(lua);
  }
  let segments = a.int("$fn").unwrap_or(16).max(2) as u32;

  let mut out: Vec<Val> = Vec::new();
  let range: Vec<usize> = if closed {
    (0..n).collect()
  } else {
    (1..n - 1).collect()
  };
  if !closed {
    out.push(Val::vec(path[0][..dim].to_vec()));
  }
  for i in range {
    let prev = path[(i + n - 1) % n];
    let here = path[i];
    let next = path[(i + 1) % n];
    let u1 = unit(sub(prev, here));
    let u2 = unit(sub(next, here));
    let cosang = dot(u1, u2).clamp(-1.0, 1.0);
    let half = cosang.acos() / 2.0;
    if half < EPS || (std::f64::consts::PI / 2.0 - half).abs() < EPS {
      out.push(Val::vec(here[..dim].to_vec()));
      continue;
    }

    // Whichever measure was given, work out how far back along each leg the
    // cut starts and what radius that implies.
    let (leg, r) = match (radius, cut, joint) {
      (Some(r), ..) => (r / half.tan(), r),
      (None, Some(c), _) => {
        let r = c * half.sin() / (1.0 - half.sin());
        (r / half.tan(), r)
      }
      (None, None, Some(j)) => (j, j * half.tan()),
      _ => unreachable!("one measure is always given"),
    };
    // Never cut back further than the neighbouring points.
    let leg = leg
      .min(norm(sub(prev, here)) / 2.0)
      .min(norm(sub(next, here)) / 2.0);
    let t1 = add(here, scale(u1, leg));
    let t2 = add(here, scale(u2, leg));

    match method.as_str() {
      "chamfer" => {
        out.push(Val::vec(t1[..dim].to_vec()));
        out.push(Val::vec(t2[..dim].to_vec()));
      }
      _ => {
        // The arc's centre sits along the bisector, far enough back that the
        // circle just touches both legs.
        let bisect = unit(add(u1, u2));
        let cp = add(here, scale(bisect, (leg * leg + r * r).sqrt().max(r)));
        let arc = arc_between(cp, t1, t2, segments);
        out.extend(arc.iter().map(|p| Val::vec(p[..dim].to_vec())));
      }
    }
  }
  if !closed {
    out.push(Val::vec(path[n - 1][..dim].to_vec()));
  }
  Val::List(out).to_lua(lua)
}

/// Points along the shorter arc from `t1` to `t2` about `cp`.
fn arc_between(
  cp: [f64; 3],
  t1: [f64; 3],
  t2: [f64; 3],
  segments: u32,
) -> Vec<[f64; 3]> {
  let v1 = sub(t1, cp);
  let v2 = sub(t2, cp);
  let r = norm(v1);
  if r < EPS {
    return vec![t1, t2];
  }
  let axis = cross(v1, v2);
  if norm(axis) < EPS {
    return vec![t1, t2];
  }
  let ang = (dot(v1, v2) / (norm(v1) * norm(v2)))
    .clamp(-1.0, 1.0)
    .acos()
    .to_degrees();
  let steps = ((segments as f64 * ang / 360.0).ceil() as u32).max(2);
  (0..=steps)
    .map(|i| {
      let t = ang * i as f64 / steps as f64;
      add(cp, Mat4::rot_by_axis(axis, t).apply(v1))
    })
    .collect()
}

/// Round a path by fitting Bézier curves through it.
fn smooth_path(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let path = crate::bosl::paths::read_path(a, "path")?;
  let dim = a
    .val("path")
    .and_then(|v| v.as_matrix())
    .and_then(|m| m.first().map(|p| p.len()))
    .unwrap_or(3)
    .clamp(2, 3);
  let closed = a.bool_or("closed", false);
  let steps = a.int("splinesteps").unwrap_or(10).max(1) as usize;
  let n = path.len();
  if n < 3 {
    return Val::list(path.iter().map(|p| Val::vec(p[..dim].to_vec())))
      .to_lua(lua);
  }

  // Handles point along the line between each point's neighbours, which is
  // what makes the joins smooth.
  let tangent = |i: usize| -> [f64; 3] {
    let prev = if i == 0 {
      if closed { path[n - 1] } else { path[0] }
    } else {
      path[i - 1]
    };
    let next = if i == n - 1 {
      if closed { path[0] } else { path[n - 1] }
    } else {
      path[i + 1]
    };
    unit(sub(next, prev))
  };
  let relsize = a.num("relsize").or_else(|| a.num("size")).unwrap_or(0.5);

  let segs = if closed { n } else { n - 1 };
  let mut out: Vec<Val> = Vec::new();
  for s in 0..segs {
    let i = s;
    let j = (s + 1) % n;
    let reach = norm(sub(path[j], path[i])) * relsize / 2.0;
    let ctrl = [
      path[i],
      add(path[i], scale(tangent(i), reach)),
      sub(path[j], scale(tangent(j), reach)),
      path[j],
    ];
    for k in 0..steps {
      let u = k as f64 / steps as f64;
      out.push(Val::vec(
        crate::bosl::beziers::bezier_at(&ctrl, u)[..dim].to_vec(),
      ));
    }
  }
  if !closed {
    out.push(Val::vec(path[n - 1][..dim].to_vec()));
  }
  Val::List(out).to_lua(lua)
}

/// Extrude an outline with a rounded or chamfered top and bottom.
fn offset_sweep(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let path = read_profile(a, "path")?;
  let height = a
    .num("height")
    .or_else(|| a.num("h"))
    .or_else(|| a.num("l"))
    .or_else(|| a.num("length"))
    .unwrap_or(1.0);
  let r = a.num_or("r", 0.0);
  let steps = a.int("steps").unwrap_or(16).max(1) as usize;
  let style = a.string("offset").unwrap_or_else(|| "round".to_string());

  // The end treatment is a quarter turn of inset against rise: at the very
  // bottom the outline is pulled in by the full radius, and by the time it
  // has risen that far it is back out to its nominal size.
  let profile = end_profile(r, steps, &style);
  let outline = |inset: f64| offset_polygon(&path, -inset);

  // Rows have to climb steadily from one end to the other, or the surface
  // doubles back on itself and encloses nothing.
  let mut rows: Vec<Vec<[f64; 3]>> = Vec::new();
  for (inset, rise) in &profile {
    rows.push(
      outline(*inset)
        .iter()
        .map(|p| [p[0], p[1], *rise])
        .collect(),
    );
  }
  for (inset, rise) in profile.iter().rev() {
    rows.push(
      outline(*inset)
        .iter()
        .map(|p| [p[0], p[1], height - rise])
        .collect(),
    );
  }

  let vnf = Vnf::vertex_array(&rows, Caps::BOTH, true, false);
  as_geometry(lua, "offset_sweep", vnf.to_node())
}

/// How much an end treatment pulls the outline in, and how far it has risen,
/// at each step of a quarter turn.
///
/// The pair starts fully inset at zero height and finishes flush at the full
/// radius, so a list of them read in order climbs steadily.
fn end_profile(r: f64, steps: usize, style: &str) -> Vec<(f64, f64)> {
  if r.abs() < EPS {
    return vec![(0.0, 0.0)];
  }
  (0..=steps)
    .map(|i| {
      let t = i as f64 / steps as f64;
      match style {
        "chamfer" => (r * (1.0 - t), r * t),
        _ => {
          let ang = std::f64::consts::FRAC_PI_2 * t;
          (r * (1.0 - ang.sin()), r * (1.0 - ang.cos()))
        }
      }
    })
    .collect()
}

/// Move every edge of a closed outline outward by `d`.
///
/// Each edge is shifted along its own normal and the neighbouring edges are
/// intersected, which keeps the corners sharp instead of rounding them off.
fn offset_polygon(path: &[[f64; 2]], d: f64) -> Vec<[f64; 2]> {
  let n = path.len();
  if n < 3 || d.abs() < EPS {
    return path.to_vec();
  }
  // Positive `d` should grow the outline whichever way it winds.
  let area: f64 = (0..n)
    .map(|i| {
      let p = path[i];
      let q = path[(i + 1) % n];
      p[0] * q[1] - q[0] * p[1]
    })
    .sum();
  let sign = if area < 0.0 { -1.0 } else { 1.0 };

  (0..n)
    .map(|i| {
      let prev = path[(i + n - 1) % n];
      let here = path[i];
      let next = path[(i + 1) % n];
      let e1 = [here[0] - prev[0], here[1] - prev[1]];
      let e2 = [next[0] - here[0], next[1] - here[1]];
      let n1 = normalize2([e1[1], -e1[0]]);
      let n2 = normalize2([e2[1], -e2[0]]);
      let bisect = normalize2([n1[0] + n2[0], n1[1] + n2[1]]);
      // The corner moves further than the edges do, by how sharp it is.
      let cosang = (bisect[0] * n1[0] + bisect[1] * n1[1]).abs().max(0.2);
      [
        here[0] + bisect[0] * d * sign / cosang,
        here[1] + bisect[1] * d * sign / cosang,
      ]
    })
    .collect()
}

fn normalize2(v: [f64; 2]) -> [f64; 2] {
  let n = (v[0] * v[0] + v[1] * v[1]).sqrt();
  if n < EPS {
    [0.0, 0.0]
  } else {
    [v[0] / n, v[1] / n]
  }
}

/// A prism with rounded vertical edges and rounded top and bottom.
fn rounded_prism(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let bottom = read_profile(a, "bottom")?;
  let top = match a.raw("top") {
    Some(_) => read_profile(a, "top")?,
    None => bottom.clone(),
  };
  if bottom.len() != top.len() {
    return a.err("the top and bottom outlines need the same point count");
  }
  let height = a
    .num("height")
    .or_else(|| a.num("h"))
    .or_else(|| a.num("l"))
    .unwrap_or(1.0);
  let joint_top = a.num_or("joint_top", 0.0);
  let joint_bot = a.num_or("joint_bot", 0.0);
  let steps = a.int("splinesteps").unwrap_or(16).max(1) as usize;

  // Each end gets a quarter turn of inset against rise, climbing steadily
  // so the surface never doubles back.
  let bot_profile = end_profile(joint_bot, steps, "round");
  let top_profile = end_profile(joint_top, steps, "round");

  let mut rows: Vec<Vec<[f64; 3]>> = Vec::new();
  for (inset, rise) in &bot_profile {
    rows.push(
      offset_polygon(&bottom, -inset)
        .iter()
        .map(|p| [p[0], p[1], *rise])
        .collect(),
    );
  }
  for (inset, rise) in top_profile.iter().rev() {
    rows.push(
      offset_polygon(&top, -inset)
        .iter()
        .map(|p| [p[0], p[1], height - rise])
        .collect(),
    );
  }

  let vnf = Vnf::vertex_array(&rows, Caps::BOTH, true, false);
  as_geometry(lua, "rounded_prism", vnf.to_node())
}

/// Widen a path into a closed outline of the given width.
fn offset_stroke(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let path = crate::bosl::paths::read_path(a, "path")?;
  let width = a.num_or("width", 1.0);
  let closed = a.bool_or("closed", false);
  if path.len() < 2 {
    return a.err("the path needs at least two points");
  }
  let flat: Vec<[f64; 2]> = path.iter().map(|p| [p[0], p[1]]).collect();

  // One side is the path offset one way, the other is it offset back, and
  // together they close into a band.
  let out: Vec<Val> = if closed {
    let outer = offset_polygon(&flat, width / 2.0);
    let inner = offset_polygon(&flat, -width / 2.0);
    outer
      .iter()
      .chain(inner.iter().rev())
      .map(|p| Val::vec(*p))
      .collect()
  } else {
    let side = |sign: f64| -> Vec<[f64; 2]> {
      flat
        .iter()
        .enumerate()
        .map(|(i, p)| {
          let prev = flat[i.saturating_sub(1)];
          let next = flat[(i + 1).min(flat.len() - 1)];
          let t = normalize2([next[0] - prev[0], next[1] - prev[1]]);
          [
            p[0] + t[1] * sign * width / 2.0,
            p[1] - t[0] * sign * width / 2.0,
          ]
        })
        .collect()
    };
    side(1.0)
      .iter()
      .chain(side(-1.0).iter().rev())
      .map(|p| Val::vec(*p))
      .collect()
  };
  Val::List(out).to_lua(lua)
}

/// Extrude an outline while offsetting it, keeping the result convex.
fn convex_offset_extrude(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let path = read_profile(a, "region").or_else(|_| read_profile(a, "path"))?;
  let height = a
    .num("height")
    .or_else(|| a.num("h"))
    .or_else(|| a.num("l"))
    .unwrap_or(1.0);
  let r = a.num_or("r", 0.0);
  let steps = a.int("steps").unwrap_or(16).max(1) as usize;

  let rows: Vec<Vec<[f64; 3]>> = (0..=steps)
    .map(|i| {
      let t = i as f64 / steps as f64;
      // A quarter circle of offset, so the wall bulges rather than steps.
      let ang = std::f64::consts::FRAC_PI_2 * t;
      let d = r * ang.sin();
      offset_polygon(&path, d)
        .iter()
        .map(|p| [p[0], p[1], height * t])
        .collect()
    })
    .collect();
  let vnf = Vnf::vertex_array(&rows, Caps::BOTH, true, false);
  as_geometry(lua, "convex_offset_extrude", vnf.to_node())
}

/// Join several paths end to end, rounding where they meet.
fn path_join(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let Some(items) =
    a.val("paths").and_then(|v| v.as_list().map(|s| s.to_vec()))
  else {
    return a.err("paths must be a list of paths");
  };
  let mut out: Vec<Val> = Vec::new();
  let mut dim = 2usize;
  for p in &items {
    let Some(m) = p.as_matrix() else {
      return a.err("every entry must be a path");
    };
    dim = dim.max(m.first().map(|q| q.len()).unwrap_or(2));
    for q in &m {
      let point = v3(q);
      // Skip a point that repeats where the previous path ended.
      let repeat = out
        .last()
        .and_then(|v| v.as_vec())
        .is_some_and(|prev| norm(sub(point, v3(&prev))) < 1e-9);
      if !repeat {
        out.push(Val::vec(point[..dim.min(3)].to_vec()));
      }
    }
  }
  Val::List(out).to_lua(lua)
}

// ---------------------------------------------------------------------------
// drawing
// ---------------------------------------------------------------------------

/// The points of a helix.
fn helix(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let length = a.num("l").or_else(|| a.num("h"));
  let turns = a.num("turns");
  let angle = a.num("angle");
  let r1 = a.radius_end("r1", "d1", "r", "d", Some(1.0)).unwrap_or(1.0);
  let r2 = a.radius_end("r2", "d2", "r", "d", Some(1.0)).unwrap_or(1.0);
  if [length.is_some(), turns.is_some(), angle.is_some()]
    .iter()
    .filter(|x| **x)
    .count()
    != 2
  {
    return a.err("give exactly two of l/h, turns and angle");
  }

  // The rise per turn follows from whichever pair was given.
  let (max_theta, dz) = match (length, turns, angle) {
    (Some(l), Some(t), _) => (360.0 * t, l / t.abs()),
    (Some(l), None, Some(ang)) => {
      let dz = std::f64::consts::TAU * r1 * ang.to_radians().tan();
      if dz.abs() < EPS {
        return a.err("that angle gives a helix with no rise");
      }
      (360.0 * l / dz, dz)
    }
    (None, Some(t), Some(ang)) => (
      360.0 * t,
      std::f64::consts::TAU * r1 * ang.to_radians().tan(),
    ),
    _ => return a.err("give exactly two of l/h, turns and angle"),
  };

  let facets = a.segments(r1.max(r2));
  let n = ((max_theta.abs() * facets as f64 / 360.0).ceil() as usize).max(3);
  let pts: Vec<Val> = (0..=n)
    .map(|i| {
      let t = i as f64 / n as f64;
      let theta = max_theta * t;
      let r = r1 + (r2 - r1) * t;
      let (s, c) = theta.to_radians().sin_cos();
      Val::vec([r * c, r * s, theta.abs() / 360.0 * dz])
    })
    .collect();
  Val::List(pts).to_lua(lua)
}

/// Draw a path as a solid of the given width.
fn stroke(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let path = crate::bosl::paths::read_path(a, "path")?;
  let width = a.num_or("width", 1.0);
  let closed = a.bool_or("closed", false);
  if path.len() < 2 {
    return a.err("the path needs at least two points");
  }
  let pts = if closed {
    path
      .iter()
      .copied()
      .chain(std::iter::once(path[0]))
      .collect()
  } else {
    path.clone()
  };

  // A bar along each segment, with a ball at each joint so the corners are
  // filled in rather than notched.
  let mut parts: Vec<ScadNode> = pts
    .windows(2)
    .map(|w| crate::bosl::vnf_lua::bar_between(w[0], w[1], width))
    .collect();
  let interior = if closed { pts.len() - 1 } else { pts.len() };
  for p in pts.iter().take(interior) {
    parts.push(ScadNode::Translate {
      x: p[0] as f32,
      y: p[1] as f32,
      z: p[2] as f32,
      child: Box::new(ScadNode::Sphere {
        r: (width / 2.0) as f32,
        segments: 12,
      }),
    });
  }
  as_geometry(lua, "stroke", ScadNode::Union(parts))
}

/// Break a path into dashes, returning the pieces.
fn dashed_stroke(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let path = crate::bosl::paths::read_path(a, "path")?;
  let dim = a
    .val("path")
    .and_then(|v| v.as_matrix())
    .and_then(|m| m.first().map(|p| p.len()))
    .unwrap_or(3)
    .clamp(2, 3);
  let closed = a.bool_or("closed", false);
  let pattern = a
    .nums("dashpat")
    .filter(|p| !p.is_empty())
    .unwrap_or_else(|| vec![3.0, 3.0]);
  if pattern.iter().any(|d| *d < 0.0) {
    return a.err("the dash pattern cannot contain negative lengths");
  }

  let pts: Vec<[f64; 3]> = if closed {
    path
      .iter()
      .copied()
      .chain(std::iter::once(path[0]))
      .collect()
  } else {
    path.clone()
  };
  let mut cum = vec![0.0];
  for w in pts.windows(2) {
    cum.push(cum[cum.len() - 1] + norm(sub(w[1], w[0])));
  }
  let total = *cum.last().unwrap_or(&0.0);
  let at = |d: f64| -> [f64; 3] {
    let d = d.clamp(0.0, total);
    let i = cum
      .iter()
      .rposition(|c| *c <= d + 1e-12)
      .unwrap_or(0)
      .min(pts.len().saturating_sub(2));
    let seg = cum[i + 1] - cum[i];
    let t = if seg < 1e-12 { 0.0 } else { (d - cum[i]) / seg };
    add(pts[i], scale(sub(pts[i + 1], pts[i]), t))
  };

  // Alternate along the pattern: the odd entries are the gaps.
  let mut dashes: Vec<Val> = Vec::new();
  let mut pos = 0.0;
  let mut k = 0usize;
  let cycle: f64 = pattern.iter().sum();
  if cycle < EPS {
    return a.err("the dash pattern has no length");
  }
  while pos < total - 1e-9 {
    let len = pattern[k % pattern.len()];
    if k.is_multiple_of(2) && len > 0.0 {
      let end = (pos + len).min(total);
      dashes.push(Val::list([
        Val::vec(at(pos)[..dim].to_vec()),
        Val::vec(at(end)[..dim].to_vec()),
      ]));
    }
    pos += len;
    k += 1;
  }
  Val::List(dashes).to_lua(lua)
}

/// An arc, as a path.
fn arc(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let cp = a
    .val("cp")
    .and_then(|v| v.as_vec())
    .map(|p| [p[0], *p.get(1).unwrap_or(&0.0)])
    .unwrap_or([0.0, 0.0]);

  // The arc may be given by an angle range, or by points it passes through.
  if let Some(pts) = a.val("points").and_then(|v| v.as_matrix())
    && pts.len() == 3
  {
    // Three points name a unique circle.
    let (p0, p1, p2) = (
      [pts[0][0], pts[0][1]],
      [pts[1][0], pts[1][1]],
      [pts[2][0], pts[2][1]],
    );
    let Some((centre, radius)) = circle_through(p0, p1, p2) else {
      return a.err("the three points are collinear");
    };
    let ang =
      |p: [f64; 2]| (p[1] - centre[1]).atan2(p[0] - centre[0]).to_degrees();
    let (a0, a1, a2) = (ang(p0), ang(p1), ang(p2));
    // Sweep the way that passes through the middle point.
    let mut sweep = a2 - a0;
    let mut through = a1 - a0;
    while sweep <= -180.0 {
      sweep += 360.0;
    }
    while sweep > 180.0 {
      sweep -= 360.0;
    }
    while through <= -180.0 {
      through += 360.0;
    }
    while through > 180.0 {
      through -= 360.0;
    }
    if sweep.signum() != through.signum() {
      sweep -= 360.0 * sweep.signum();
    }
    let n = a.int("n").map(|v| v.max(2) as u32).unwrap_or_else(|| {
      crate::bosl::shapes2d::arc_n(a.segments(radius), sweep)
    });
    return Val::list(
      arc_pts(n, radius, centre, a0, sweep, true)
        .iter()
        .map(|p| Val::vec(*p)),
    )
    .to_lua(lua);
  }

  let r = match a.radius("r", "d", None) {
    Some(r) => r,
    None => return a.err("give a radius, a diameter, or three points"),
  };
  let (start, sweep) = match a.val("angle") {
    Some(Val::Num(ang)) => (a.num_or("start", 0.0), ang),
    Some(other) => match other.as_vec() {
      Some(v) if v.len() >= 2 => (v[0], v[1] - v[0]),
      _ => return a.err("angle must be a sweep or a [start, end] pair"),
    },
    None => (a.num_or("start", 0.0), 360.0),
  };
  let n = a
    .int("n")
    .map(|v| v.max(2) as u32)
    .unwrap_or_else(|| crate::bosl::shapes2d::arc_n(a.segments(r), sweep));
  let mut pts: Vec<Val> = Vec::new();
  if a.bool_or("wedge", false) {
    pts.push(Val::vec(cp));
  }
  pts.extend(
    arc_pts(n, r, cp, start, sweep, a.bool_or("endpoint", true))
      .iter()
      .map(|p| Val::vec(*p)),
  );
  Val::List(pts).to_lua(lua)
}

/// The circle through three points.
fn circle_through(
  p0: [f64; 2],
  p1: [f64; 2],
  p2: [f64; 2],
) -> Option<([f64; 2], f64)> {
  let d = 2.0
    * (p0[0] * (p1[1] - p2[1])
      + p1[0] * (p2[1] - p0[1])
      + p2[0] * (p0[1] - p1[1]));
  if d.abs() < EPS {
    return None;
  }
  let s0 = p0[0] * p0[0] + p0[1] * p0[1];
  let s1 = p1[0] * p1[0] + p1[1] * p1[1];
  let s2 = p2[0] * p2[0] + p2[1] * p2[1];
  let cx =
    (s0 * (p1[1] - p2[1]) + s1 * (p2[1] - p0[1]) + s2 * (p0[1] - p1[1])) / d;
  let cy =
    (s0 * (p2[0] - p1[0]) + s1 * (p0[0] - p2[0]) + s2 * (p1[0] - p0[0])) / d;
  let r = ((p0[0] - cx).powi(2) + (p0[1] - cy).powi(2)).sqrt();
  Some(([cx, cy], r))
}

// ---------------------------------------------------------------------------
// Registration
// ---------------------------------------------------------------------------

fn register_one(
  lua: &Lua,
  bosl: &mlua::Table,
  name: &'static str,
  params: &'static [&'static str],
  f: fn(&Lua, &Args) -> LuaResult<LuaValue>,
) -> LuaResult<()> {
  let func = lua.create_function(move |lua, args: mlua::MultiValue| {
    let parsed = Args::parse_pure(name, params, &args)?;
    f(lua, &parsed)
  })?;
  bosl.set(name, func)?;
  Ok(())
}

pub fn register(lua: &Lua, bosl: &mlua::Table) -> LuaResult<()> {
  register_one(
    lua,
    bosl,
    "skin",
    &[
      "profiles", "slices", "refine", "method", "sampling", "caps", "closed",
      "z", "style", "anchor", "cp", "spin", "orient", "atype",
    ],
    skin,
  )?;
  register_one(
    lua,
    bosl,
    "path_sweep",
    &[
      "shape",
      "path",
      "method",
      "normal",
      "closed",
      "twist",
      "twist_by_length",
      "scale",
      "scale_by_length",
      "symmetry",
      "last_normal",
      "tangent",
      "uniform",
      "relaxed",
      "caps",
      "style",
    ],
    path_sweep,
  )?;
  register_one(
    lua,
    bosl,
    "path_sweep2d",
    &["shape", "path", "closed", "caps", "quality", "style"],
    path_sweep2d,
  )?;
  register_one(
    lua,
    bosl,
    "sweep",
    &["shape", "transforms", "closed", "caps", "style"],
    sweep,
  )?;
  register_one(
    lua,
    bosl,
    "linear_sweep",
    &[
      "region", "height", "center", "twist", "scale", "shift", "slices",
      "maxseg", "style", "caps", "cp", "atype", "h", "l",
    ],
    linear_sweep,
  )?;
  register_one(
    lua,
    bosl,
    "spiral_sweep",
    &[
      "poly", "h", "r", "turns", "taper", "r1", "r2", "d", "d1", "d2",
      "internal", "height", "l", "length", "anchor", "spin", "orient",
    ],
    spiral_sweep,
  )?;

  register_one(
    lua,
    bosl,
    "round_corners",
    &[
      "path", "method", "radius", "r", "cut", "joint", "width", "k", "closed",
      "verbose", "$fn",
    ],
    round_corners,
  )?;
  register_one(
    lua,
    bosl,
    "smooth_path",
    &[
      "path",
      "tangents",
      "size",
      "relsize",
      "method",
      "splinesteps",
      "uniform",
      "closed",
    ],
    smooth_path,
  )?;
  register_one(
    lua,
    bosl,
    "offset_sweep",
    &[
      "path",
      "height",
      "bottom",
      "top",
      "h",
      "l",
      "length",
      "ends",
      "bot",
      "offset",
      "r",
      "steps",
      "quality",
      "check_valid",
      "extra",
    ],
    offset_sweep,
  )?;
  register_one(
    lua,
    bosl,
    "rounded_prism",
    &[
      "bottom",
      "top",
      "height",
      "h",
      "l",
      "joint_top",
      "joint_bot",
      "joint_sides",
      "k",
      "k_top",
      "k_bot",
      "k_sides",
      "splinesteps",
      "debug",
      "convexity",
    ],
    rounded_prism,
  )?;
  register_one(
    lua,
    bosl,
    "offset_stroke",
    &[
      "path",
      "width",
      "rounded",
      "start",
      "end",
      "check_valid",
      "quality",
      "chamfer",
      "closed",
      "atype",
      "anchor",
      "spin",
      "cp",
    ],
    offset_stroke,
  )?;
  register_one(
    lua,
    bosl,
    "convex_offset_extrude",
    &[
      "height",
      "h",
      "l",
      "center",
      "region",
      "path",
      "twist",
      "slices",
      "scale",
      "offset",
      "chamfer",
      "r",
      "steps",
      "check_valid",
      "quality",
    ],
    convex_offset_extrude,
  )?;
  register_one(
    lua,
    bosl,
    "path_join",
    &["paths", "joint", "k", "relocate", "closed"],
    path_join,
  )?;

  register_one(
    lua,
    bosl,
    "helix",
    &["l", "h", "turns", "angle", "r", "r1", "r2", "d", "d1", "d2"],
    helix,
  )?;
  register_one(
    lua,
    bosl,
    "stroke",
    &[
      "path", "width", "closed", "endcaps", "joints", "dots", "color",
    ],
    stroke,
  )?;
  register_one(
    lua,
    bosl,
    "dashed_stroke",
    &["path", "dashpat", "closed", "fit", "mindash", "width"],
    dashed_stroke,
  )?;
  register_one(
    lua,
    bosl,
    "arc",
    &[
      "n",
      "r",
      "angle",
      "d",
      "cp",
      "points",
      "corner",
      "width",
      "thickness",
      "start",
      "wedge",
      "long",
      "cw",
      "ccw",
      "endpoint",
    ],
    arc,
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

  const SQUARE: &str = "{{-5,-5},{5,-5},{5,5},{-5,5}}";

  #[test]
  fn skinning_two_squares_makes_a_box() {
    let v = volume(&format!(
      "local a, b = {{}}, {{}}
       for i, p in ipairs({SQUARE}) do
         a[i] = {{p[1], p[2], 0}}
         b[i] = {{p[1], p[2], 10}}
       end
       render(bosl.skin({{a, b}}, 0))"
    ));
    assert!((v - 1000.0).abs() < 1e-3, "{v}");
  }

  #[test]
  fn skinning_to_a_smaller_profile_makes_a_frustum() {
    let v = volume(
      "local a, b = {}, {}
       for i, p in ipairs({{-10,-10},{10,-10},{10,10},{-10,10}}) do
         a[i] = {p[1], p[2], 0}
         b[i] = {p[1]/2, p[2]/2, 12}
       end
       render(bosl.skin({a, b}, 0))",
    );
    let ideal = 12.0 / 3.0 * (400.0 + 100.0 + (400.0f64 * 100.0).sqrt());
    assert!((v - ideal).abs() < 1.0, "{v} vs {ideal}");
  }

  #[test]
  fn sweeping_a_square_along_a_straight_path_makes_a_bar() {
    let v = volume(&format!(
      "render(bosl.path_sweep({SQUARE}, {{{{0,0,0}},{{0,0,20}}}}))"
    ));
    assert!((v - 100.0 * 20.0).abs() < 1e-3, "{v}");
  }

  #[test]
  fn a_swept_profile_follows_a_bent_path() {
    let (lo, hi) = bbox(&format!(
      "render(bosl.path_sweep({SQUARE},
        {{{{0,0,0}},{{0,0,20}},{{20,0,20}}}}))"
    ));
    assert!(hi[0] > 15.0, "{hi:?}");
    assert!(hi[2] > 15.0, "{hi:?}");
    assert!(lo[2] < 1.0, "{lo:?}");
  }

  #[test]
  fn a_linear_sweep_extrudes_its_outline() {
    let v = volume(&format!("render(bosl.linear_sweep({SQUARE}, 8))"));
    assert!((v - 800.0).abs() < 1e-3, "{v}");
  }

  #[test]
  fn a_twisted_sweep_still_has_the_same_volume() {
    let plain = volume(&format!("render(bosl.linear_sweep({SQUARE}, 20))"));
    let twisted = volume(&format!(
      "render(bosl.linear_sweep({{region = {SQUARE}, height = 20,
                                  twist = 90, slices = 36}}))"
    ));
    assert!((twisted / plain - 1.0).abs() < 0.02, "{twisted} vs {plain}");
  }

  #[test]
  fn centering_a_linear_sweep_straddles_the_plane() {
    let (lo, hi) = bbox(&format!(
      "render(bosl.linear_sweep({{region = {SQUARE}, height = 10,
                                  center = true}}))"
    ));
    assert!((lo[2] + 5.0).abs() < 1e-3, "{lo:?}");
    assert!((hi[2] - 5.0).abs() < 1e-3, "{hi:?}");
  }

  #[test]
  fn a_spiral_sweep_wraps_a_profile_round_an_axis() {
    let (lo, hi) = bbox(
      "render(bosl.spiral_sweep({{-1,-1},{1,-1},{1,1},{-1,1}},
                                {h = 20, r = 10, turns = 2}))",
    );
    assert!((hi[0] - 11.0).abs() < 0.5, "{hi:?}");
    assert!((lo[0] + 11.0).abs() < 0.5, "{lo:?}");
    assert!(hi[2] > 9.0 && lo[2] < -9.0, "{lo:?} {hi:?}");
  }

  #[test]
  fn rounding_corners_replaces_them_with_arcs() {
    let n: usize =
      eval(&format!("return #bosl.round_corners({SQUARE}, {{r = 2}})"));
    assert!(n > 4, "{n}");
    // The rounded outline is smaller than the square it came from.
    let area: f64 = eval(&format!(
      "return bosl.polygon_area(bosl.round_corners({SQUARE}, {{r = 2}}))"
    ));
    assert!(area < 100.0 && area > 90.0, "{area}");
  }

  #[test]
  fn chamfering_corners_cuts_them_straight_across() {
    let n: usize = eval(&format!(
      "return #bosl.round_corners({SQUARE}, {{method = 'chamfer', r = 2}})"
    ));
    assert_eq!(n, 8);
  }

  #[test]
  fn smoothing_a_path_passes_near_its_points_and_adds_more() {
    let n: usize = eval(
      "return #bosl.smooth_path({{0,0},{10,10},{20,0}}, {splinesteps = 8})",
    );
    assert!(n > 3, "{n}");
  }

  #[test]
  fn an_offset_sweep_rounds_the_top_and_bottom() {
    let plain = volume(&format!("render(bosl.linear_sweep({SQUARE}, 10))"));
    let rounded = volume(&format!(
      "render(bosl.offset_sweep({{path = {SQUARE}, height = 10, r = 2}}))"
    ));
    assert!(rounded < plain, "{rounded} vs {plain}");
    assert!(rounded > plain * 0.8, "{rounded} vs {plain}");
  }

  #[test]
  fn a_rounded_prism_is_smaller_than_the_box_it_starts_from() {
    let v = volume(&format!(
      "render(bosl.rounded_prism({{bottom = {SQUARE}, height = 10,
                                   joint_top = 2, joint_bot = 2}}))"
    ));
    assert!(v < 1000.0 && v > 800.0, "{v}");
  }

  #[test]
  fn a_stroke_draws_a_solid_along_its_path() {
    let (lo, hi) = bbox("render(bosl.stroke({{0,0,0},{20,0,0}}, 2))");
    assert!((hi[0] - 21.0).abs() < 0.2, "{hi:?}");
    assert!((lo[0] + 1.0).abs() < 0.2, "{lo:?}");
    assert!((hi[1] - 1.0).abs() < 0.2, "{hi:?}");
  }

  #[test]
  fn a_dashed_stroke_breaks_the_path_into_pieces() {
    let dashes: Vec<Vec<Vec<f64>>> =
      eval("return bosl.dashed_stroke({{0,0},{20,0}}, {5, 5})");
    assert_eq!(dashes.len(), 2);
    assert_eq!(dashes[0][0][0], 0.0);
    assert_eq!(dashes[0][1][0], 5.0);
    assert_eq!(dashes[1][0][0], 10.0);
  }

  #[test]
  fn a_helix_rises_by_its_length_over_its_turns() {
    let pts: Vec<Vec<f64>> =
      eval("return bosl.helix({l = 20, turns = 2, r = 10})");
    assert!((pts[0][2]).abs() < 1e-9, "{:?}", pts[0]);
    let last = &pts[pts.len() - 1];
    assert!((last[2] - 20.0).abs() < 1e-6, "{last:?}");
    // Every point sits on the given radius.
    for p in &pts {
      assert!(((p[0] * p[0] + p[1] * p[1]).sqrt() - 10.0).abs() < 1e-6);
    }
  }

  #[test]
  fn a_tapered_helix_changes_radius_as_it_climbs() {
    let pts: Vec<Vec<f64>> =
      eval("return bosl.helix({l = 20, turns = 2, r1 = 10, r2 = 5})");
    let r = |p: &Vec<f64>| (p[0] * p[0] + p[1] * p[1]).sqrt();
    assert!((r(&pts[0]) - 10.0).abs() < 1e-6);
    assert!((r(&pts[pts.len() - 1]) - 5.0).abs() < 1e-6);
  }

  #[test]
  fn an_arc_covers_the_angle_it_is_given() {
    let pts: Vec<Vec<f64>> = eval("return bosl.arc({r = 10, angle = {0, 90}})");
    assert!((pts[0][0] - 10.0).abs() < 1e-9, "{pts:?}");
    let last = &pts[pts.len() - 1];
    assert!((last[1] - 10.0).abs() < 1e-9, "{last:?}");
  }

  #[test]
  fn an_arc_through_three_points_passes_through_all_of_them() {
    let pts: Vec<Vec<f64>> =
      eval("return bosl.arc({points = {{10,0},{0,10},{-10,0}}})");
    assert!((pts[0][0] - 10.0).abs() < 1e-6, "{pts:?}");
    let last = &pts[pts.len() - 1];
    assert!((last[0] + 10.0).abs() < 1e-6, "{last:?}");
    // Every point is on the circle through them.
    for p in &pts {
      assert!(((p[0] * p[0] + p[1] * p[1]).sqrt() - 10.0).abs() < 1e-6);
    }
  }

  #[test]
  fn joining_paths_drops_the_repeated_join_point() {
    let p: Vec<Vec<f64>> =
      eval("return bosl.path_join({{{0,0},{10,0}}, {{10,0},{10,10}}})");
    assert_eq!(p.len(), 3);
  }

  #[test]
  fn offset_stroke_widens_a_path_into_an_outline() {
    let area: f64 =
      eval("return bosl.polygon_area(bosl.offset_stroke({{0,0},{20,0}}, 4))");
    assert!((area - 80.0).abs() < 1.0, "{area}");
  }

  #[test]
  fn a_sweep_through_explicit_transforms_follows_them() {
    let v = volume(&format!(
      "local mats = {{}}
       for i = 0, 4 do mats[i+1] = bosl.up(i * 5) end
       render(bosl.sweep({SQUARE}, mats))"
    ));
    assert!((v - 100.0 * 20.0).abs() < 1e-3, "{v}");
  }
}
