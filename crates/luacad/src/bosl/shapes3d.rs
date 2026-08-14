//! Native implementations of BOSL2's 3D shapes.
//!
//! Most of these are a profile swept around the Z axis — a cylinder, a cone,
//! a tube, a torus, a sphere all differ only in the profile — so they share
//! [`crate::bosl::vnf::Vnf::rotate_sweep`]. The box-like shapes are lofted
//! between two outlines from [`crate::bosl::shapes2d`], and `cuboid` builds
//! its rounded and chamfered edges by masking.

use mlua::Result as LuaResult;

use crate::bosl::args::Args;
use crate::bosl::attach::{Attachable, Geom, reorient, reorient_default};
use crate::bosl::edges::{self, EdgeSet, edge_index, edge_vector, other_axes};
use crate::bosl::shapes2d::{Path, rect_path, teardrop2d_path};
use crate::bosl::vecmath::{Mat4, V2, V3};
use crate::bosl::vnf::{Caps, Vnf, arc_pts, ccw};
use crate::scad_export::ScadNode;

const EPS: f64 = 1e-9;

/// A shape builder returns `None` when the arguments ask for something the
/// native implementation does not cover, so the call falls back to OpenSCAD
/// rather than quietly producing the wrong solid.
type Build = fn(&Args) -> LuaResult<Option<ScadNode>>;

// ---------------------------------------------------------------------------
// Profile helpers
// ---------------------------------------------------------------------------

/// Round the corner at `b` between legs `a`–`b`–`c` with radius `r`.
///
/// Returns the arc's points, tangent to both legs. A radius that does not fit
/// on either leg is clamped so the fillet still lands on the shape.
pub fn corner_arc(a: V2, b: V2, c: V2, r: f64, full_circle: u32) -> Vec<V2> {
  let u1 = unit2([a[0] - b[0], a[1] - b[1]]);
  let u2 = unit2([c[0] - b[0], c[1] - b[1]]);
  let cosang = (u1[0] * u2[0] + u1[1] * u2[1]).clamp(-1.0, 1.0);
  let half = cosang.acos() / 2.0;
  if half <= EPS || (std::f64::consts::PI / 2.0 - half).abs() < EPS && r == 0.0
  {
    return vec![b];
  }
  let tan_half = half.tan();
  if tan_half.abs() < EPS {
    return vec![b];
  }
  let t = r.abs() / tan_half;
  let t1 = [b[0] + u1[0] * t, b[1] + u1[1] * t];
  let t2 = [b[0] + u2[0] * t, b[1] + u2[1] * t];
  let bisect = unit2([u1[0] + u2[0], u1[1] + u2[1]]);
  let dist = r.abs() / half.sin();
  let cp = [b[0] + bisect[0] * dist, b[1] + bisect[1] * dist];

  let a1 = (t1[1] - cp[1]).atan2(t1[0] - cp[0]).to_degrees();
  let a2 = (t2[1] - cp[1]).atan2(t2[0] - cp[0]).to_degrees();
  let mut sweep = a2 - a1;
  while sweep > 180.0 {
    sweep -= 360.0;
  }
  while sweep < -180.0 {
    sweep += 360.0;
  }
  // BOSL2 gives an arc `max(3, ceil(segs(r) * angle/360))` points, based on
  // the arc's own radius rather than the shape's — a small fillet on a large
  // cylinder gets a coarse arc, and matching that is what keeps the volumes
  // in step.
  let n = ((full_circle as f64 * sweep.abs() / 360.0).ceil() as u32).max(3);
  arc_pts(n, r.abs(), cp, a1, sweep, true)
}

fn unit2(v: V2) -> V2 {
  let n = (v[0] * v[0] + v[1] * v[1]).sqrt();
  if n < EPS {
    [0.0, 0.0]
  } else {
    [v[0] / n, v[1] / n]
  }
}

/// How a cylinder's end is finished.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct EndTreatment {
  pub chamfer: f64,
  pub chamfang: Option<f64>,
  pub rounding: f64,
  pub from_end: bool,
}

impl EndTreatment {
  fn is_plain(&self) -> bool {
    self.chamfer.abs() < EPS && self.rounding.abs() < EPS
  }
}

/// The `(radius, z)` profile of a cylinder or cone, including its end
/// treatments, ready to be revolved about Z.
pub fn cyl_profile(
  r1: f64,
  r2: f64,
  l: f64,
  end1: EndTreatment,
  end2: EndTreatment,
  segs: &dyn Fn(f64) -> u32,
) -> Result<Path, String> {
  // The cone's wall leans by this much, which both chamfer angles and
  // rounding lengths are measured against.
  let vang = (r1 - r2).atan2(l).to_degrees();
  let mut path: Path = vec![[0.0, -l / 2.0]];

  // --- bottom end ---
  if !end1.is_plain() && end1.chamfer.abs() >= EPS {
    let chang = end1
      .chamfang
      .unwrap_or(45.0 + end1.chamfer.signum() * vang / 2.0);
    let (cr, cl) =
      chamfer_legs(end1.chamfer, chang, vang, end1.from_end, true)?;
    if cr > r1 + EPS {
      return Err("chamfer1 is larger than the r1 radius".into());
    }
    path.push([r1 - cr, -l / 2.0]);
    let a = (90.0 + vang).to_radians();
    path.push([r1 + cl * a.cos(), -l / 2.0 + cl * a.sin()]);
  } else if end1.rounding.abs() >= EPS {
    let rl = rounding_leg(end1.rounding, vang, true);
    if rl > r1 + EPS {
      return Err("rounding1 is larger than the r1 radius".into());
    }
    path.extend(corner_arc(
      [(r1 - 2.0 * rl).max(0.0), -l / 2.0],
      [r1, -l / 2.0],
      [r2, l / 2.0],
      end1.rounding,
      segs(end1.rounding.abs()),
    ));
  } else {
    path.push([r1, -l / 2.0]);
  }

  // --- top end ---
  if !end2.is_plain() && end2.chamfer.abs() >= EPS {
    let chang = end2
      .chamfang
      .unwrap_or(45.0 - end2.chamfer.signum() * vang / 2.0);
    let (cr, cl) =
      chamfer_legs(end2.chamfer, chang, vang, end2.from_end, false)?;
    if cr > r2 + EPS {
      return Err("chamfer2 is larger than the r2 radius".into());
    }
    let a = (270.0 + vang).to_radians();
    path.push([r2 + cl * a.cos(), l / 2.0 + cl * a.sin()]);
    path.push([r2 - cr, l / 2.0]);
  } else if end2.rounding.abs() >= EPS {
    let rl = rounding_leg(end2.rounding, vang, false);
    if rl > r2 + EPS {
      return Err("rounding2 is larger than the r2 radius".into());
    }
    path.extend(corner_arc(
      [r1, -l / 2.0],
      [r2, l / 2.0],
      [(r2 - 2.0 * rl).max(0.0), l / 2.0],
      end2.rounding,
      segs(end2.rounding.abs()),
    ));
  } else {
    path.push([r2, l / 2.0]);
  }

  path.push([0.0, l / 2.0]);
  Ok(path)
}

/// How far a chamfer reaches inward and along the wall.
fn chamfer_legs(
  chamfer: f64,
  chang: f64,
  vang: f64,
  from_end: bool,
  bottom: bool,
) -> Result<(f64, f64), String> {
  if chang <= 0.0 {
    return Err("chamfang must be positive".into());
  }
  let sign = chamfer.signum();
  let wall = if bottom {
    90.0 - sign * vang
  } else {
    90.0 + sign * vang
  };
  if chang >= 180.0 - wall {
    return Err("chamfang must be smaller than the cone face angle".into());
  }
  // The chamfer, the wall and the end face form a triangle; the law of sines
  // turns the given side into the other two.
  let third = 180.0 - chang - wall;
  let sin = |d: f64| d.to_radians().sin();
  if from_end {
    let cr = chamfer * sin(chang) / sin(third);
    Ok((cr, chamfer.abs()))
  } else {
    let cl = (chamfer * sin(third) / sin(chang)).abs();
    Ok((chamfer, cl))
  }
}

/// How far a rounding reaches along the wall.
fn rounding_leg(rounding: f64, vang: f64, bottom: bool) -> f64 {
  let half = if bottom {
    if rounding >= 0.0 {
      45.0 - vang / 2.0
    } else {
      45.0 + vang / 2.0
    }
  } else if rounding >= 0.0 {
    45.0 + vang / 2.0
  } else {
    45.0 - vang / 2.0
  };
  (rounding / half.to_radians().tan()).abs()
}

/// Read the `x`, `x1` and `x2` family of end parameters.
fn end_treatments(args: &Args) -> (EndTreatment, EndTreatment) {
  let pick = |base: &str, one: &str| args.num(one).or_else(|| args.num(base));
  let chamfer1 = args
    .num("chamfer1")
    .or_else(|| {
      args
        .num("rounding1")
        .is_none()
        .then(|| args.num("chamfer"))
        .flatten()
    })
    .unwrap_or(0.0);
  let chamfer2 = args
    .num("chamfer2")
    .or_else(|| {
      args
        .num("rounding2")
        .is_none()
        .then(|| args.num("chamfer"))
        .flatten()
    })
    .unwrap_or(0.0);
  let rounding1 = args
    .num("rounding1")
    .or_else(|| {
      args
        .num("chamfer1")
        .is_none()
        .then(|| args.num("rounding"))
        .flatten()
    })
    .unwrap_or(0.0);
  let rounding2 = args
    .num("rounding2")
    .or_else(|| {
      args
        .num("chamfer2")
        .is_none()
        .then(|| args.num("rounding"))
        .flatten()
    })
    .unwrap_or(0.0);
  (
    EndTreatment {
      chamfer: chamfer1,
      chamfang: pick("chamfang", "chamfang1"),
      rounding: rounding1,
      from_end: args
        .bool("from_end1")
        .or_else(|| args.bool("from_end"))
        .unwrap_or(false),
    },
    EndTreatment {
      chamfer: chamfer2,
      chamfang: pick("chamfang", "chamfang2"),
      rounding: rounding2,
      from_end: args
        .bool("from_end2")
        .or_else(|| args.bool("from_end"))
        .unwrap_or(false),
    },
  )
}

/// The height, under any of the four names BOSL2 accepts for it.
fn height(args: &Args, default: Option<f64>) -> Option<f64> {
  args
    .num("h")
    .or_else(|| args.num("l"))
    .or_else(|| args.num("length"))
    .or_else(|| args.num("height"))
    .or(default)
}

/// The anchor a shape falls back on when it is asked to sit on the XY plane.
const BOT: V3 = [0.0, 0.0, -1.0];
/// The anchor most shapes fall back on when nothing at all is given.
const CTR: V3 = [0.0, 0.0, 0.0];

// ---------------------------------------------------------------------------
// Cylinders
// ---------------------------------------------------------------------------

/// Build a cylinder, cone or prism about the Z axis.
///
/// `sides` fixes the facet count for a prism; `radii` overrides the `r`/`d`
/// family for the shapes that size themselves differently.
fn cylinder_like(
  args: &Args,
  sides: Option<u32>,
  radii: Option<(f64, f64)>,
) -> LuaResult<Option<ScadNode>> {
  if args.has("texture") {
    return Ok(None);
  }
  let l = height(args, Some(1.0)).unwrap_or(1.0);
  let (r1, r2) = match radii {
    Some(rr) => rr,
    None => (
      args
        .radius_end("r1", "d1", "r", "d", Some(1.0))
        .unwrap_or(1.0),
      args
        .radius_end("r2", "d2", "r", "d", Some(1.0))
        .unwrap_or(1.0),
    ),
  };
  let facets = sides.unwrap_or_else(|| args.segments(r1.max(r2)));
  let circum = args.bool_or("circum", false);
  let sc = if circum {
    1.0 / (std::f64::consts::PI / facets as f64).cos()
  } else {
    1.0
  };
  let (r1, r2) = (r1 * sc, r2 * sc);

  let (end1, end2) = end_treatments(args);
  // A teardrop end is a different profile shape, not just a rounding.
  if args.has("teardrop") && args.bool("teardrop") != Some(false) {
    return Ok(None);
  }

  let profile = cyl_profile(r1, r2, l, end1, end2, &|r| args.segments(r))
    .map_err(|e| err(args, e))?;
  let mut node = Vnf::rotate_sweep(&profile, 360.0, facets, false).to_node();

  if args.bool_or("realign", false) {
    node = ScadNode::Rotate {
      x: 0.0,
      y: 0.0,
      z: (180.0 / facets as f64) as f32,
      child: Box::new(node),
    };
  }

  // A shift leans the top of the cylinder sideways without tilting its ends.
  // The shear is anchored at the bottom face, so the base stays where it is
  // and the whole `shift` shows up at the top.
  let shift = args.vec2("shift").unwrap_or([0.0, 0.0]);
  if shift[0] != 0.0 || shift[1] != 0.0 {
    let skew = Mat4([
      1.0,
      0.0,
      shift[0] / l,
      0.0, //
      0.0,
      1.0,
      shift[1] / l,
      0.0, //
      0.0,
      0.0,
      1.0,
      0.0, //
      0.0,
      0.0,
      0.0,
      1.0,
    ]);
    node = crate::bosl::attach::transform(
      node,
      Mat4::translate([0.0, 0.0, -l / 2.0])
        .mul(&skew)
        .mul(&Mat4::translate([0.0, 0.0, l / 2.0])),
    );
  }

  let attachable = Attachable::new(Geom::Conoid {
    r1: [r1, r1],
    r2: [r2, r2],
    l,
    shift,
    axis: [0.0, 0.0, 1.0],
  });
  // A cylinder straddles the XY plane unless asked otherwise, but
  // `center = false` stands it on the plane.
  Ok(Some(reorient_default(node, args, &attachable, BOT, CTR)?))
}

const CYL_PARAMS: &[&str] = &[
  "h",
  "r",
  "center",
  "l",
  "r1",
  "r2",
  "d",
  "d1",
  "d2",
  "chamfer",
  "chamfer1",
  "chamfer2",
  "chamfang",
  "chamfang1",
  "chamfang2",
  "rounding",
  "rounding1",
  "rounding2",
  "circum",
  "realign",
  "shift",
  "teardrop",
  "from_end",
  "from_end1",
  "from_end2",
  "length",
  "height",
  "texture",
  "tex_size",
  "tex_reps",
  "tex_depth",
  "tex_inset",
  "tex_rot",
  "tex_samples",
  "tex_taper",
  "style",
];

fn build_cyl(args: &Args) -> LuaResult<Option<ScadNode>> {
  cylinder_like(args, None, None)
}

/// A cylinder laid along an axis other than Z.
fn axial_cyl(args: &Args, axis: V3) -> LuaResult<Option<ScadNode>> {
  let Some(node) = cylinder_like(args, None, None)? else {
    return Ok(None);
  };
  Ok(Some(crate::bosl::attach::transform(
    node,
    Mat4::rot_from_to([0.0, 0.0, 1.0], axis),
  )))
}

fn build_xcyl(args: &Args) -> LuaResult<Option<ScadNode>> {
  axial_cyl(args, [1.0, 0.0, 0.0])
}

fn build_ycyl(args: &Args) -> LuaResult<Option<ScadNode>> {
  axial_cyl(args, [0.0, 1.0, 0.0])
}

fn build_zcyl(args: &Args) -> LuaResult<Option<ScadNode>> {
  cylinder_like(args, None, None)
}

fn build_regular_prism(args: &Args) -> LuaResult<Option<ScadNode>> {
  let Some(n) = args.int("n").map(|v| v as u32) else {
    return Err(err(args, "n is required".to_string()));
  };
  if n < 3 {
    return Err(err(args, "n must be at least 3".to_string()));
  }
  // A prism can be sized by the circle through its corners, the circle
  // touching its faces, or the length of a side; the sweep only knows the
  // first, so the other two are converted to it.
  let sc = 1.0 / (std::f64::consts::PI / n as f64).cos();
  let side_to_r = 1.0 / (2.0 * (std::f64::consts::PI / n as f64).sin());
  let end = |suffix: &str| -> Option<f64> {
    if let Some(v) =
      args.radius(&format!("ir{suffix}"), &format!("id{suffix}"), None)
    {
      return Some(v * sc);
    }
    if let Some(v) =
      args.radius(&format!("or{suffix}"), &format!("od{suffix}"), None)
    {
      return Some(v);
    }
    if let Some(v) = args.num(&format!("side{suffix}")) {
      return Some(v * side_to_r);
    }
    args.radius(&format!("r{suffix}"), &format!("d{suffix}"), None)
  };
  let shared = end("").unwrap_or(1.0);
  let r1 = end("1").unwrap_or(shared);
  let r2 = end("2").unwrap_or(shared);
  cylinder_like(args, Some(n), Some((r1, r2)))
}

fn build_tube(args: &Args) -> LuaResult<Option<ScadNode>> {
  let h = height(args, Some(1.0)).unwrap_or(1.0);
  let wall = args.num("wall");
  let outer1 = args.radius_end("or1", "od1", "or", "od", None);
  let outer2 = args.radius_end("or2", "od2", "or", "od", None);
  let inner1 = args.radius_end("ir1", "id1", "ir", "id", None);
  let inner2 = args.radius_end("ir2", "id2", "ir", "id", None);

  let combine = |outer: Option<f64>, inner: Option<f64>| match (outer, inner) {
    (Some(o), Some(i)) => Some((o, i)),
    (Some(o), None) => wall.map(|w| (o, o - w)),
    (None, Some(i)) => wall.map(|w| (i + w, i)),
    (None, None) => None,
  };
  let (Some((r1, ir1)), Some((r2, ir2))) =
    (combine(outer1, inner1), combine(outer2, inner2))
  else {
    return Err(err(
      args,
      "give two of the inner radius, the outer radius and the wall thickness"
        .to_string(),
    ));
  };
  if ir1 > r1 || ir2 > r2 {
    return Err(err(
      args,
      "the inner radius is larger than the outer one".to_string(),
    ));
  }

  let facets = args.segments(r1.max(r2));
  let outer = Vnf::rotate_sweep(
    &cyl_profile(
      r1,
      r2,
      h,
      ends_of(args, false).0,
      ends_of(args, false).1,
      &|r| args.segments(r),
    )
    .map_err(|e| err(args, e))?,
    360.0,
    facets,
    false,
  )
  .to_node();
  // The bore is made slightly longer so its end faces never land exactly on
  // the tube's, which would leave a zero-thickness sliver in the boolean.
  let bore = Vnf::rotate_sweep(
    &cyl_profile(
      ir1,
      ir2,
      h + 0.01,
      ends_of(args, true).0,
      ends_of(args, true).1,
      &|r| args.segments(r),
    )
    .map_err(|e| err(args, e))?,
    360.0,
    facets,
    false,
  )
  .to_node();

  let node = ScadNode::Difference(vec![outer, bore]);
  let attachable = Attachable::new(Geom::Conoid {
    r1: [r1, r1],
    r2: [r2, r2],
    l: h,
    shift: [0.0, 0.0],
    axis: [0.0, 0.0, 1.0],
  });
  Ok(Some(reorient_default(node, args, &attachable, BOT, CTR)?))
}

/// A tube's inner and outer walls take their own rounding and chamfer, with
/// the inner ones cut the opposite way.
fn ends_of(args: &Args, inner: bool) -> (EndTreatment, EndTreatment) {
  let sign = if inner { -1.0 } else { 1.0 };
  let prefix = if inner { "i" } else { "o" };
  let pick = |kind: &str, end: &str| {
    args
      .num(&format!("{prefix}{kind}{end}"))
      .or_else(|| args.num(&format!("{kind}{end}")))
      .or_else(|| args.num(&format!("{prefix}{kind}")))
      .or_else(|| args.num(kind))
  };
  let make = |end: &str| EndTreatment {
    chamfer: pick("chamfer", end).unwrap_or(0.0) * sign,
    chamfang: None,
    rounding: pick("rounding", end).unwrap_or(0.0) * sign,
    from_end: false,
  };
  (make("1"), make("2"))
}

const TUBE_PARAMS: &[&str] = &[
  "h",
  "or",
  "ir",
  "center",
  "od",
  "id",
  "wall",
  "or1",
  "or2",
  "od1",
  "od2",
  "ir1",
  "ir2",
  "id1",
  "id2",
  "realign",
  "l",
  "length",
  "height",
  "orounding1",
  "irounding1",
  "orounding2",
  "irounding2",
  "rounding1",
  "rounding2",
  "rounding",
  "ochamfer1",
  "ichamfer1",
  "ochamfer2",
  "ichamfer2",
  "chamfer1",
  "chamfer2",
  "chamfer",
  "irounding",
  "ichamfer",
  "orounding",
  "ochamfer",
  "teardrop",
];

fn build_pie_slice(args: &Args) -> LuaResult<Option<ScadNode>> {
  let l = height(args, Some(1.0)).unwrap_or(1.0);
  let r1 = args
    .radius_end("r1", "d1", "r", "d", Some(10.0))
    .unwrap_or(10.0);
  let r2 = args
    .radius_end("r2", "d2", "r", "d", Some(10.0))
    .unwrap_or(10.0);
  let ang = args.num_or("ang", 30.0);
  if ang <= 0.0 || ang > 360.0 {
    return Err(err(args, "ang must be between 0 and 360".to_string()));
  }
  let facets = args.segments(r1.max(r2));
  // Sweeping the profile through the wedge angle gives the slice directly,
  // with a facet count scaled to the fraction of the turn it covers.
  let arc_facets = ((facets as f64 * ang / 360.0).ceil() as u32).max(1);
  let profile = vec![
    [0.0, -l / 2.0],
    [r1, -l / 2.0],
    [r2, l / 2.0],
    [0.0, l / 2.0],
  ];
  let node = Vnf::rotate_sweep(&profile, ang, arc_facets, true).to_node();
  let attachable = Attachable::new(Geom::Conoid {
    r1: [r1, r1],
    r2: [r2, r2],
    l,
    shift: [0.0, 0.0],
    axis: [0.0, 0.0, 1.0],
  });
  Ok(Some(reorient_default(node, args, &attachable, BOT, BOT)?))
}

// ---------------------------------------------------------------------------
// Round shapes
// ---------------------------------------------------------------------------

fn build_spheroid(args: &Args) -> LuaResult<Option<ScadNode>> {
  let r = args.radius("r", "d", Some(1.0)).unwrap_or(1.0);
  let sides = args.segments(r);
  let vsides = sides.div_ceil(2).max(2);
  let circum = args.bool_or("circum", false);
  let sc = if circum {
    1.0 / (std::f64::consts::PI / sides as f64).cos()
  } else {
    1.0
  };
  // A half-circle from pole to pole, revolved into the sphere.
  let profile: Path = (0..=vsides)
    .map(|i| {
      let a = -90.0 + 180.0 * i as f64 / vsides as f64;
      let (s, c) = a.to_radians().sin_cos();
      [r * sc * c, r * sc * s]
    })
    .collect();
  let node = Vnf::rotate_sweep(&profile, 360.0, sides, false).to_node();
  let node = reorient(
    node,
    args,
    &Attachable::new(Geom::Spheroid {
      r: [r * sc, r * sc, r * sc],
    }),
  )?;
  Ok(Some(node))
}

fn build_torus(args: &Args) -> LuaResult<Option<ScadNode>> {
  let outer = args.radius("or", "od", None);
  let inner = args.radius("ir", "id", None);
  let maj = args.radius("r_maj", "d_maj", None);
  let min = args.radius("r_min", "d_min", None);

  let maj_rad = match (maj, inner, outer, min) {
    (Some(m), ..) => m,
    (None, Some(i), Some(o), _) => (o + i) / 2.0,
    (None, Some(i), None, Some(m)) => i + m,
    (None, None, Some(o), Some(m)) => o - m,
    _ => {
      return Err(err(args, "give two of r_maj, r_min, or and ir".to_string()));
    }
  };
  let min_rad = match (min, inner, outer) {
    (Some(m), ..) => m,
    (None, Some(i), _) => maj_rad - i,
    (None, None, Some(o)) => o - maj_rad,
    _ => {
      return Err(err(args, "give two of r_maj, r_min, or and ir".to_string()));
    }
  };
  if min_rad <= 0.0 || maj_rad <= 0.0 {
    return Err(err(args, "the torus radii must be positive".to_string()));
  }

  let minor_facets = args.segments(min_rad);
  let major_facets = args.segments(maj_rad + min_rad);
  let profile: Path = (0..minor_facets)
    .map(|i| {
      let a = 360.0 * i as f64 / minor_facets as f64;
      let (s, c) = a.to_radians().sin_cos();
      [maj_rad + min_rad * c, min_rad * s]
    })
    .collect();
  let node = Vnf::rotate_sweep(&profile, 360.0, major_facets, true).to_node();
  let attachable = Attachable::new(Geom::Conoid {
    r1: [maj_rad + min_rad, maj_rad + min_rad],
    r2: [maj_rad + min_rad, maj_rad + min_rad],
    l: min_rad * 2.0,
    shift: [0.0, 0.0],
    axis: [0.0, 0.0, 1.0],
  });
  Ok(Some(reorient_default(node, args, &attachable, BOT, CTR)?))
}

fn build_onion(args: &Args) -> LuaResult<Option<ScadNode>> {
  let r = args.radius("r", "d", Some(1.0)).unwrap_or(1.0);
  let ang = args.num_or("ang", 45.0);
  let sides = args.segments(r);
  let profile = teardrop2d_path(
    r,
    ang,
    args.num("cap_h"),
    sides,
    args.bool_or("realign", false),
  )
  .map_err(|e| err(args, e))?;

  // Only the right half is revolved, so the outline is cut at x = 0. The
  // sweep expects a counter-clockwise profile, and `teardrop2d` draws its
  // outline the other way round.
  let half = ccw(clip_right_half(&profile));
  if half.len() < 3 {
    return Err(err(args, "the teardrop profile is degenerate".to_string()));
  }
  let node = Vnf::rotate_sweep(&half, 360.0, sides, true).to_node();
  let tip = profile.iter().fold(f64::NEG_INFINITY, |a, p| a.max(p[1]));
  let cap_h = args.num("cap_h").map(|h| h.min(tip)).unwrap_or(tip);
  let attachable = Attachable::new(Geom::Spheroid { r: [r, r, r] })
    .with_named("cap", [0.0, 0.0, cap_h])
    .with_named("tip", [0.0, 0.0, tip]);
  Ok(Some(reorient(node, args, &attachable)?))
}

/// Keep the part of a closed outline with x >= 0, cutting edges that cross.
fn clip_right_half(path: &[V2]) -> Path {
  let n = path.len();
  let mut out = Path::new();
  for i in 0..n {
    let a = path[i];
    let b = path[(i + 1) % n];
    if a[0] >= -EPS {
      out.push(a);
    }
    if (a[0] > EPS && b[0] < -EPS) || (a[0] < -EPS && b[0] > EPS) {
      let t = a[0] / (a[0] - b[0]);
      out.push([0.0, a[1] + (b[1] - a[1]) * t]);
    }
  }
  crate::bosl::shapes2d::dedup_closed(out)
}

fn build_teardrop(args: &Args) -> LuaResult<Option<ScadNode>> {
  let l = height(args, None);
  let Some(l) = l else {
    return Err(err(args, "a length is required".to_string()));
  };
  if l <= 0.0 {
    return Err(err(args, "the length must be positive".to_string()));
  }
  let r1 = args
    .radius_end("r1", "d1", "r", "d", Some(1.0))
    .unwrap_or(1.0);
  let r2 = args
    .radius_end("r2", "d2", "r", "d", Some(1.0))
    .unwrap_or(1.0);
  let ang = args.num_or("ang", 45.0);
  let sides = args.segments(r1.max(r2));
  let realign = args.bool_or("realign", false);

  let cap = |r: f64, which: &str| {
    args
      .num(which)
      .or_else(|| args.num("cap_h"))
      .map(|h| h.min(r / (90.0 - ang).to_radians().cos()))
  };
  let p1 = teardrop2d_path(r1, ang, cap(r1, "cap_h1"), sides, realign)
    .map_err(|e| err(args, e))?;
  let p2 = teardrop2d_path(r2, ang, cap(r2, "cap_h2"), sides, realign)
    .map_err(|e| err(args, e))?;
  if p1.len() != p2.len() {
    return Ok(None);
  }

  // The profile lies in XZ and the body runs along Y, matching BOSL2's
  // `axis = BACK`.
  let section = |p: &Path, y: f64| -> Vec<V3> {
    ccw(p.clone()).iter().map(|q| [q[0], y, q[1]]).collect()
  };
  let vnf =
    Vnf::skin(&[section(&p1, -l / 2.0), section(&p2, l / 2.0)], Caps::BOTH);

  let tip1 = r1 / (90.0 - ang).to_radians().cos();
  let tip2 = r2 / (90.0 - ang).to_radians().cos();
  let cap_h1 = cap(r1, "cap_h1").unwrap_or(tip1);
  let cap_h2 = cap(r2, "cap_h2").unwrap_or(tip2);
  let attachable = Attachable::new(Geom::Conoid {
    r1: [r1, r1],
    r2: [r2, r2],
    l,
    shift: [0.0, 0.0],
    axis: [0.0, 1.0, 0.0],
  })
  .with_named("cap", [0.0, 0.0, (cap_h1 + cap_h2) / 2.0])
  .with_named("cap_fwd", [0.0, -l / 2.0, cap_h1])
  .with_named("cap_back", [0.0, l / 2.0, cap_h2]);
  Ok(Some(reorient(vnf.to_node(), args, &attachable)?))
}

// ---------------------------------------------------------------------------
// Box-like shapes
// ---------------------------------------------------------------------------

fn build_prismoid(args: &Args) -> LuaResult<Option<ScadNode>> {
  let size1 = args.vec2("size1").or_else(|| args.vec2("size"));
  let size2 = args.vec2("size2").or_else(|| args.vec2("size"));
  let (Some(s1), Some(s2)) = (size1, size2) else {
    return Err(err(args, "size1 and size2 are required".to_string()));
  };
  let Some(h) = height(args, None) else {
    return Err(err(args, "a height is required".to_string()));
  };
  if h <= 0.0 {
    return Err(err(args, "the height must be positive".to_string()));
  }
  if s1[0] + s2[0] <= 0.0 || s1[1] + s2[1] <= 0.0 {
    return Err(err(args, "degenerate prismoid geometry".to_string()));
  }
  let shift = args.vec2("shift").unwrap_or([0.0, 0.0]);

  let corners = |base: &str, end: &str| -> [f64; 4] {
    let read = |name: &str| match args.nums(name) {
      Some(v) if v.len() == 4 => Some([v[0], v[1], v[2], v[3]]),
      _ => args.num(name).map(|v| [v; 4]),
    };
    read(end).or_else(|| read(base)).unwrap_or([0.0; 4])
  };
  let round1 = corners("rounding", "rounding1");
  let round2 = corners("rounding", "rounding2");
  let cham1 = corners("chamfer", "chamfer1");
  let cham2 = corners("chamfer", "chamfer2");

  let segs = args.segments(
    round1
      .iter()
      .chain(round2.iter())
      .fold(0.0f64, |a, v| a.max(v.abs())),
  );
  let bottom = rect_path(s1, round1, cham1, segs).map_err(|e| err(args, e))?;
  let top = rect_path(s2, round2, cham2, segs).map_err(|e| err(args, e))?;

  // The two ends can have different corner treatments and so different point
  // counts; hulling them avoids having to match the outlines up.
  let node = if bottom.len() == top.len() {
    let lower: Vec<V3> = ccw(bottom.clone())
      .iter()
      .map(|p| [p[0], p[1], -h / 2.0])
      .collect();
    let upper: Vec<V3> = ccw(top.clone())
      .iter()
      .map(|p| [p[0] + shift[0], p[1] + shift[1], h / 2.0])
      .collect();
    Vnf::skin(&[lower, upper], Caps::BOTH).to_node()
  } else {
    ScadNode::Hull(Box::new(ScadNode::Union(vec![
      ScadNode::Translate {
        x: 0.0,
        y: 0.0,
        z: (-h / 2.0) as f32,
        child: Box::new(crate::bosl::shapes2d::path_node(&bottom)),
      },
      ScadNode::Translate {
        x: shift[0] as f32,
        y: shift[1] as f32,
        z: (h / 2.0) as f32,
        child: Box::new(crate::bosl::shapes2d::path_node(&top)),
      },
    ])))
  };

  let attachable = Attachable::new(Geom::Prismoid {
    size: [s1[0], s1[1], h],
    size2: s2,
    shift,
    axis: [0.0, 0.0, 1.0],
  });
  Ok(Some(reorient_default(node, args, &attachable, BOT, BOT)?))
}

fn build_rect_tube(args: &Args) -> LuaResult<Option<ScadNode>> {
  let Some(h) = height(args, None) else {
    return Err(err(args, "a height is required".to_string()));
  };
  let wall = args.num("wall");
  let pick2 = |a: &str, b: &str| args.vec2(a).or_else(|| args.vec2(b));
  let s1 = pick2("size1", "size");
  let s2 = pick2("size2", "size");
  let is1 = pick2("isize1", "isize");
  let is2 = pick2("isize2", "isize");

  let resolve = |outer: Option<V2>, inner: Option<V2>| match (outer, inner) {
    (Some(o), Some(i)) => Some((o, i)),
    (Some(o), None) => wall.map(|w| (o, [o[0] - 2.0 * w, o[1] - 2.0 * w])),
    (None, Some(i)) => wall.map(|w| ([i[0] + 2.0 * w, i[1] + 2.0 * w], i)),
    (None, None) => None,
  };
  let (Some((size1, isize1)), Some((size2, isize2))) =
    (resolve(s1, is1), resolve(s2, is2))
  else {
    return Err(err(
      args,
      "give the outer size, the inner size, or one of them with a wall"
        .to_string(),
    ));
  };
  if isize1[0] >= size1[0]
    || isize1[1] >= size1[1]
    || isize2[0] >= size2[0]
    || isize2[1] >= size2[1]
  {
    return Err(err(
      args,
      "the inner size is larger than the outer size".to_string(),
    ));
  }
  let shift = args.vec2("shift").unwrap_or([0.0, 0.0]);

  let corners = |base: &str, end: &str| -> [f64; 4] {
    let read = |name: &str| match args.nums(name) {
      Some(v) if v.len() == 4 => Some([v[0], v[1], v[2], v[3]]),
      _ => args.num(name).map(|v| [v; 4]),
    };
    read(end).or_else(|| read(base)).unwrap_or([0.0; 4])
  };
  let segs = args.segments(
    [
      corners("rounding", "rounding1"),
      corners("rounding", "rounding2"),
    ]
    .iter()
    .flatten()
    .fold(0.0f64, |a, v| a.max(v.abs())),
  );

  let solid = |sz1: V2,
               sz2: V2,
               r1: [f64; 4],
               r2: [f64; 4],
               c1: [f64; 4],
               c2: [f64; 4],
               dh: f64|
   -> Result<ScadNode, String> {
    let bottom = rect_path(sz1, r1, c1, segs)?;
    let top = rect_path(sz2, r2, c2, segs)?;
    if bottom.len() != top.len() {
      return Err("the two ends need matching corner treatments".into());
    }
    let lower: Vec<V3> = ccw(bottom)
      .iter()
      .map(|p| [p[0], p[1], -dh / 2.0])
      .collect();
    let upper: Vec<V3> = ccw(top)
      .iter()
      .map(|p| [p[0] + shift[0], p[1] + shift[1], dh / 2.0])
      .collect();
    Ok(Vnf::skin(&[lower, upper], Caps::BOTH).to_node())
  };

  // The default inner corner rounding follows the wall thickness, keeping
  // the wall an even thickness around the corner.
  let inner_default = |outer: [f64; 4], sz: V2, isz: V2| -> [f64; 4] {
    let w = ((sz[0] - isz[0]) / 2.0).min((sz[1] - isz[1]) / 2.0);
    std::array::from_fn(|i| (outer[i] - w).max(0.0))
  };
  let or1 = corners("rounding", "rounding1");
  let or2 = corners("rounding", "rounding2");
  let ir1 = match args.raw("irounding").or_else(|| args.raw("irounding1")) {
    Some(_) => corners("irounding", "irounding1"),
    None => inner_default(or1, size1, isize1),
  };
  let ir2 = match args.raw("irounding").or_else(|| args.raw("irounding2")) {
    Some(_) => corners("irounding", "irounding2"),
    None => inner_default(or2, size2, isize2),
  };
  let oc1 = corners("chamfer", "chamfer1");
  let oc2 = corners("chamfer", "chamfer2");
  let ic1 = match args.raw("ichamfer").or_else(|| args.raw("ichamfer1")) {
    Some(_) => corners("ichamfer", "ichamfer1"),
    None => inner_default(oc1, size1, isize1),
  };
  let ic2 = match args.raw("ichamfer").or_else(|| args.raw("ichamfer2")) {
    Some(_) => corners("ichamfer", "ichamfer2"),
    None => inner_default(oc2, size2, isize2),
  };

  let outer =
    solid(size1, size2, or1, or2, oc1, oc2, h).map_err(|e| err(args, e))?;
  // The bore runs slightly past both ends so the difference leaves no film.
  let bore = solid(isize1, isize2, ir1, ir2, ic1, ic2, h + 0.02)
    .map_err(|e| err(args, e))?;

  let node = ScadNode::Difference(vec![outer, bore]);
  let attachable = Attachable::new(Geom::Prismoid {
    size: [size1[0], size1[1], h],
    size2,
    shift,
    axis: [0.0, 0.0, 1.0],
  });
  Ok(Some(reorient_default(node, args, &attachable, BOT, BOT)?))
}

fn build_wedge(args: &Args) -> LuaResult<Option<ScadNode>> {
  let size = args.vec3("size").unwrap_or([1.0, 1.0, 1.0]);
  let s = [size[0] / 2.0, size[1] / 2.0, size[2] / 2.0];
  let points = [
    [s[0], s[1], -s[2]],
    [s[0], -s[1], -s[2]],
    [s[0], -s[1], s[2]],
    [-s[0], s[1], -s[2]],
    [-s[0], -s[1], -s[2]],
    [-s[0], -s[1], s[2]],
  ];
  let faces = vec![
    vec![0, 1, 2],
    vec![3, 5, 4],
    vec![0, 3, 1],
    vec![1, 3, 4],
    vec![1, 4, 2],
    vec![2, 4, 5],
    vec![2, 5, 3],
    vec![0, 2, 3],
  ];
  // BOSL2's own face list is in OpenSCAD's winding, so it goes straight into
  // a polyhedron rather than through the outward-facing VNF convention.
  let node = ScadNode::Polyhedron {
    points: points
      .iter()
      .map(|p| [p[0] as f32, p[1] as f32, p[2] as f32])
      .collect(),
    faces,
  };
  let attachable = Attachable::new(Geom::Prismoid {
    size,
    size2: [size[0], size[1]],
    shift: [0.0, 0.0],
    axis: [0.0, 0.0, 1.0],
  })
  .with_named("hypot", [0.0, 0.0, 0.0])
  .with_named("hypot_left", [-size[0] / 2.0, 0.0, 0.0])
  .with_named("hypot_right", [size[0] / 2.0, 0.0, 0.0]);

  // A wedge's default anchor is its front-bottom-left corner.
  let corner = [-1.0, -1.0, -1.0];
  Ok(Some(reorient_default(
    node,
    args,
    &attachable,
    corner,
    corner,
  )?))
}

fn build_octahedron(args: &Args) -> LuaResult<Option<ScadNode>> {
  let size = args.vec3("size").unwrap_or([1.0, 1.0, 1.0]);
  let s = [size[0] / 2.0, size[1] / 2.0, size[2] / 2.0];
  let points = vec![
    [0.0, 0.0, s[2]],
    [s[0], 0.0, 0.0],
    [0.0, s[1], 0.0],
    [-s[0], 0.0, 0.0],
    [0.0, -s[1], 0.0],
    [0.0, 0.0, -s[2]],
  ];
  let faces = vec![
    vec![0, 2, 1],
    vec![0, 3, 2],
    vec![0, 4, 3],
    vec![0, 1, 4],
    vec![5, 1, 2],
    vec![5, 2, 3],
    vec![5, 3, 4],
    vec![5, 4, 1],
  ];
  let node = ScadNode::Polyhedron {
    points: points
      .iter()
      .map(|p| [p[0] as f32, p[1] as f32, p[2] as f32])
      .collect(),
    faces,
  };
  let attachable = Attachable::new(Geom::VnfExtent { points });
  Ok(Some(reorient(node, args, &attachable)?))
}

// ---------------------------------------------------------------------------
// cuboid
// ---------------------------------------------------------------------------

fn build_cuboid(args: &Args) -> LuaResult<Option<ScadNode>> {
  let size = args.vec3("size").unwrap_or([1.0, 1.0, 1.0]);
  if size.iter().any(|c| *c < 0.0) {
    return Err(err(args, "all components of size must be >= 0".to_string()));
  }
  // A teardrop cuboid replaces its rounded edges with printable ones, which
  // is a different profile than a fillet.
  if args.has("teardrop") && args.bool("teardrop") != Some(false) {
    return Ok(None);
  }

  let chamfer = args.num("chamfer").filter(|v| v.abs() >= EPS);
  let rounding = args.num("rounding").filter(|v| v.abs() >= EPS);
  if chamfer.is_some() && rounding.is_some() {
    return Err(err(
      args,
      "cannot give both a chamfer and a rounding".to_string(),
    ));
  }
  let selected = edges::from_args(args)?;
  let trimcorners = args.bool_or("trimcorners", true);

  let body = match (chamfer, rounding) {
    (None, None) => plain_box(size),
    (Some(c), None) => cut_box(args, size, c, selected, trimcorners, true)?,
    (None, Some(r)) => cut_box(args, size, r, selected, trimcorners, false)?,
    (Some(_), Some(_)) => unreachable!("both were rejected above"),
  };

  // `p1`/`p2` place the box by two opposite corners instead of by size.
  let node = match (args.vec3("p1"), args.vec3("p2")) {
    (Some(p1), Some(p2)) => {
      let lo = [p1[0].min(p2[0]), p1[1].min(p2[1]), p1[2].min(p2[2])];
      let sz = [
        (p2[0] - p1[0]).abs(),
        (p2[1] - p1[1]).abs(),
        (p2[2] - p1[2]).abs(),
      ];
      let body = match (chamfer, rounding) {
        (None, None) => plain_box(sz),
        (Some(c), None) => cut_box(args, sz, c, selected, trimcorners, true)?,
        (None, Some(r)) => cut_box(args, sz, r, selected, trimcorners, false)?,
        _ => unreachable!(),
      };
      return Ok(Some(crate::bosl::attach::transform(
        body,
        Mat4::translate([
          lo[0] + sz[0] / 2.0,
          lo[1] + sz[1] / 2.0,
          lo[2] + sz[2] / 2.0,
        ]),
      )));
    }
    (Some(p1), None) => {
      return Ok(Some(crate::bosl::attach::transform(
        body,
        Mat4::translate([
          p1[0] + size[0] / 2.0,
          p1[1] + size[1] / 2.0,
          p1[2] + size[2] / 2.0,
        ]),
      )));
    }
    _ => body,
  };

  let attachable = Attachable::new(Geom::Prismoid {
    size,
    size2: [size[0], size[1]],
    shift: [0.0, 0.0],
    axis: [0.0, 0.0, 1.0],
  });
  Ok(Some(reorient(node, args, &attachable)?))
}

fn plain_box(size: V3) -> ScadNode {
  ScadNode::Cube {
    w: size[0] as f32,
    d: size[1] as f32,
    h: size[2] as f32,
    center: true,
  }
}

/// A box with its selected edges rounded or chamfered.
fn cut_box(
  args: &Args,
  size: V3,
  amount: f64,
  selected: EdgeSet,
  trimcorners: bool,
  chamfer: bool,
) -> LuaResult<ScadNode> {
  if selected.is_empty() {
    return Ok(plain_box(size));
  }
  check_fits(args, size, amount.abs(), selected)?;
  let segments = args.segments(amount.abs());

  // Every edge and corner cut away at once is exactly the convex hull of the
  // shapes left in the corners, which is both faster and cleaner than a
  // dozen boolean subtractions.
  if amount > 0.0 && trimcorners && selected == edges::EDGES_ALL {
    return Ok(if chamfer {
      hull_of_chamfered_box(size, amount)
    } else {
      hull_of_rounded_box(size, amount, segments)
    });
  }

  let mut masks: Vec<ScadNode> = Vec::new();
  for (axis, i) in selected.iter() {
    masks.push(edge_mask(axis, i, size, amount, chamfer, segments));
  }
  if trimcorners {
    for corner in corners() {
      if selected.corner_is_full(corner) {
        masks.push(corner_mask(corner, size, amount, chamfer, segments));
      }
    }
  }

  Ok(if amount > 0.0 {
    ScadNode::Difference(
      std::iter::once(plain_box(size)).chain(masks).collect(),
    )
  } else {
    // A negative amount adds a fillet around the outside of the edge
    // instead of cutting into it.
    ScadNode::Union(std::iter::once(plain_box(size)).chain(masks).collect())
  })
}

/// Reject a rounding that would eat through the box.
fn check_fits(
  args: &Args,
  size: V3,
  amount: f64,
  selected: EdgeSet,
) -> LuaResult<()> {
  for axis in 0..3 {
    // An edge parallel to `axis` cuts into the other two directions, so two
    // opposite edges must together fit inside each of those spans.
    let (u, v) = other_axes(axis);
    for (dim, other) in [(u, v), (v, u)] {
      let mut worst: f64 = 0.0;
      for sign_other in [-1.0, 1.0] {
        let mut span = 0.0;
        for sign_dim in [-1.0, 1.0] {
          let mut vec = [0.0; 3];
          vec[dim] = sign_dim;
          vec[other] = sign_other;
          let i = edge_index(vec[other_axes(axis).0], vec[other_axes(axis).1]);
          if selected.0[axis][i] {
            span += amount;
          }
        }
        worst = worst.max(span);
      }
      if worst > size[dim] + EPS {
        return Err(err(
          args,
          format!(
            "the rounding or chamfer is too large for the cuboid in the {} axis",
            ["X", "Y", "Z"][dim]
          ),
        ));
      }
    }
  }
  Ok(())
}

/// A box with every edge rounded, as the hull of eight corner spheres.
fn hull_of_rounded_box(size: V3, r: f64, segments: u32) -> ScadNode {
  let inset = [size[0] / 2.0 - r, size[1] / 2.0 - r, size[2] / 2.0 - r];
  let spheres: Vec<ScadNode> = corners()
    .into_iter()
    .map(|c| ScadNode::Translate {
      x: (c[0] * inset[0]) as f32,
      y: (c[1] * inset[1]) as f32,
      z: (c[2] * inset[2]) as f32,
      child: Box::new(ScadNode::Sphere {
        r: r as f32,
        segments,
      }),
    })
    .collect();
  ScadNode::Hull(Box::new(ScadNode::Union(spheres)))
}

/// A box with every edge chamfered, as the hull of three inset boxes.
fn hull_of_chamfered_box(size: V3, c: f64) -> ScadNode {
  let inset: V3 = std::array::from_fn(|i| (size[i] - 2.0 * c).max(0.001));
  ScadNode::Hull(Box::new(ScadNode::Union(vec![
    plain_box([size[0], inset[1], inset[2]]),
    plain_box([inset[0], size[1], inset[2]]),
    plain_box([inset[0], inset[1], size[2]]),
  ])))
}

fn corners() -> Vec<V3> {
  let mut out = Vec::with_capacity(8);
  for x in [-1.0, 1.0] {
    for y in [-1.0, 1.0] {
      for z in [-1.0, 1.0] {
        out.push([x, y, z]);
      }
    }
  }
  out
}

/// The material to remove along one edge to round or chamfer it.
///
/// The mask is a prism running the length of the edge whose cross-section is
/// the corner square minus the fillet, so subtracting it leaves the rounded
/// edge and nothing else.
fn edge_mask(
  axis: usize,
  i: usize,
  size: V3,
  amount: f64,
  chamfer: bool,
  segments: u32,
) -> ScadNode {
  let r = amount.abs();
  let ev = edge_vector(axis, i);
  let (u, v) = other_axes(axis);

  // The cross-section is drawn with the box corner at the origin and the
  // material to cut lying in the third quadrant, then extruded along Z.
  let profile = corner_cut_profile(r, chamfer, amount > 0.0, segments);
  let solid = ScadNode::LinearExtrude {
    height: (size[axis] + 2.0) as f32,
    center: true,
    twist: 0.0,
    slices: 1,
    scale: 1.0,
    child: Box::new(crate::bosl::shapes2d::path_node(&profile)),
  };

  // Map the profile's own axes onto the edge: X onto the first spanned axis
  // pointing outward, Y onto the second, Z along the edge. Building the
  // frame directly is what keeps the cut on the inside of the box —
  // composing an axis rotation with a sign flip mirrors the profile for the
  // X and Y edges, which throws the mask outside the solid entirely.
  let mut basis = [[0.0; 3]; 3];
  basis[0][u] = ev[u];
  basis[1][v] = ev[v];
  basis[2][axis] = 1.0;
  // A mirrored frame would turn the extrusion inside out, so the edge
  // direction absorbs the sign instead. The mask is symmetric along the
  // edge, so which way it points makes no difference to the result.
  if determinant(&basis) < 0.0 {
    basis[2][axis] = -1.0;
  }

  let mut place = [0.0; 3];
  place[u] = ev[u] * size[u] / 2.0;
  place[v] = ev[v] * size[v] / 2.0;

  // Each basis vector becomes a column of the matrix.
  let mut m = Mat4::identity();
  for (col, axis_vec) in basis.iter().enumerate() {
    for (row, component) in axis_vec.iter().enumerate() {
      m.0[row * 4 + col] = *component;
    }
  }
  for (row, offset) in place.iter().enumerate() {
    m.0[row * 4 + 3] = *offset;
  }

  crate::bosl::attach::transform(solid, m)
}

fn determinant(b: &[[f64; 3]; 3]) -> f64 {
  b[0][0] * (b[1][1] * b[2][2] - b[1][2] * b[2][1])
    - b[0][1] * (b[1][0] * b[2][2] - b[1][2] * b[2][0])
    + b[0][2] * (b[1][0] * b[2][1] - b[1][1] * b[2][0])
}

/// The corner region left over once a fillet or chamfer is taken out of it.
///
/// For a cut the profile is the square `[-r,0]^2` minus the quarter round;
/// for an added fillet it is the mirror image, the material that fills the
/// outside of the corner.
fn corner_cut_profile(r: f64, chamfer: bool, cut: bool, segments: u32) -> Path {
  let arc = if chamfer {
    vec![[-r, 0.0], [0.0, -r]]
  } else {
    let n = (segments / 4).max(2);
    arc_pts(n + 1, r, [-r, -r], 90.0, -90.0, true)
  };
  let mut path = vec![[0.0, 0.0]];
  path.extend(arc);
  let path = crate::bosl::shapes2d::dedup_closed(path);
  if cut {
    path
  } else {
    // The added fillet is the same arc reflected through the corner.
    path.into_iter().map(|p| [-p[0], -p[1]]).collect()
  }
}

/// The material to remove at a corner where three cut edges meet.
///
/// The three edge masks alone leave the corner as the intersection of three
/// cylinders. Taking the corner cube minus a sphere out as well trims that
/// down to the sphere octant that `trimcorners` asks for.
fn corner_mask(
  corner: V3,
  size: V3,
  amount: f64,
  chamfer: bool,
  segments: u32,
) -> ScadNode {
  let r = amount.abs();
  let centre: V3 = std::array::from_fn(|i| corner[i] * (size[i] / 2.0 - r));
  let cube_centre: V3 =
    std::array::from_fn(|i| corner[i] * (size[i] / 2.0 - r / 2.0));

  let keep = if chamfer {
    // The plane through the three edge cuts.
    let d = r * 3f64.sqrt();
    ScadNode::Translate {
      x: centre[0] as f32,
      y: centre[1] as f32,
      z: centre[2] as f32,
      child: Box::new(crate::bosl::attach::transform(
        ScadNode::Cube {
          w: (2.0 * d) as f32,
          d: (2.0 * d) as f32,
          h: (2.0 * d) as f32,
          center: true,
        },
        Mat4::rot_from_to([0.0, 0.0, 1.0], corner).mul(&Mat4::translate([
          0.0,
          0.0,
          -d + r / 3f64.sqrt(),
        ])),
      )),
    }
  } else {
    ScadNode::Translate {
      x: centre[0] as f32,
      y: centre[1] as f32,
      z: centre[2] as f32,
      child: Box::new(ScadNode::Sphere {
        r: r as f32,
        segments,
      }),
    }
  };

  ScadNode::Difference(vec![
    ScadNode::Translate {
      x: cube_centre[0] as f32,
      y: cube_centre[1] as f32,
      z: cube_centre[2] as f32,
      child: Box::new(ScadNode::Cube {
        w: r as f32,
        d: r as f32,
        h: r as f32,
        center: true,
      }),
    },
    keep,
  ])
}

fn err(args: &Args, msg: String) -> mlua::Error {
  mlua::Error::RuntimeError(format!("bosl.{}(): {msg}", args.func()))
}

// ---------------------------------------------------------------------------
// Registration
// ---------------------------------------------------------------------------

/// The parameter list and builder for a 3D shape, if it has a native one.
pub fn builder(name: &str) -> Option<(&'static [&'static str], Build)> {
  Some(match name {
    "cuboid" => (
      &[
        "size",
        "p1",
        "p2",
        "chamfer",
        "rounding",
        "edges",
        "except",
        "except_edges",
        "trimcorners",
        "teardrop",
      ],
      build_cuboid as Build,
    ),
    "prismoid" => (
      &[
        "size1",
        "size2",
        "h",
        "shift",
        "xang",
        "yang",
        "rounding",
        "rounding1",
        "rounding2",
        "chamfer",
        "chamfer1",
        "chamfer2",
        "l",
        "height",
        "length",
        "center",
        "size",
      ],
      build_prismoid as Build,
    ),
    "regular_prism" => (REGULAR_PRISM_PARAMS, build_regular_prism as Build),
    "rect_tube" => (
      &[
        "h",
        "size",
        "isize",
        "center",
        "shift",
        "wall",
        "size1",
        "size2",
        "isize1",
        "isize2",
        "rounding",
        "rounding1",
        "rounding2",
        "irounding",
        "irounding1",
        "irounding2",
        "chamfer",
        "chamfer1",
        "chamfer2",
        "ichamfer",
        "ichamfer1",
        "ichamfer2",
        "l",
        "length",
        "height",
      ],
      build_rect_tube as Build,
    ),
    "wedge" => (&["size", "center"], build_wedge as Build),
    "octahedron" => (&["size"], build_octahedron as Build),
    "cyl" => (CYL_PARAMS, build_cyl as Build),
    "xcyl" => (CYL_PARAMS, build_xcyl as Build),
    "ycyl" => (CYL_PARAMS, build_ycyl as Build),
    "zcyl" => (CYL_PARAMS, build_zcyl as Build),
    "tube" => (TUBE_PARAMS, build_tube as Build),
    "pie_slice" => (
      &[
        "h", "r", "ang", "center", "r1", "r2", "d", "d1", "d2", "l", "length",
        "height",
      ],
      build_pie_slice as Build,
    ),
    "spheroid" => (
      &["r", "style", "d", "circum", "dual"],
      build_spheroid as Build,
    ),
    "torus" => (
      &[
        "r_maj", "r_min", "center", "d_maj", "d_min", "or", "od", "ir", "id",
      ],
      build_torus as Build,
    ),
    "teardrop" => (
      &[
        "h", "r", "ang", "cap_h", "r1", "r2", "d", "d1", "d2", "cap_h1",
        "cap_h2", "l", "length", "height", "circum", "realign", "chamfer",
        "chamfer1", "chamfer2",
      ],
      build_teardrop as Build,
    ),
    "onion" => (
      &["r", "ang", "cap_h", "d", "circum", "realign"],
      build_onion as Build,
    ),
    "fillet" => (
      &[
        "l",
        "r",
        "ang",
        "r1",
        "r2",
        "excess",
        "d1",
        "d2",
        "d",
        "length",
        "h",
        "height",
        "overlap",
        "rounding",
        "rounding1",
        "rounding2",
        "chamfer",
        "chamfer1",
        "chamfer2",
      ],
      build_fillet as Build,
    ),
    "text3d" => (
      &[
        "text",
        "h",
        "size",
        "font",
        "spacing",
        "direction",
        "language",
        "script",
        "height",
        "thickness",
        "atype",
        "center",
      ],
      build_text3d as Build,
    ),
    "path_text" => (
      &[
        "path",
        "text",
        "font",
        "size",
        "thickness",
        "lettersize",
        "offset",
        "reverse",
        "normal",
        "top",
        "center",
        "textmetrics",
        "kern",
        "height",
        "h",
        "valign",
        "language",
        "script",
      ],
      build_path_text as Build,
    ),
    _ => return None,
  })
}

// ---------------------------------------------------------------------------
// Fillets
// ---------------------------------------------------------------------------

/// The cross-section of a fillet between two faces meeting at `ang`.
///
/// The corner sits at the origin with one face along +X and the other turned
/// `ang` away from it. The arc is tangent to both, and `excess` pushes the
/// outline a little past each face so the fillet always meets solid material
/// rather than sitting exactly on the surface.
fn fillet_profile(r: f64, ang: f64, excess: f64, steps: u32) -> Path {
  let half = (ang / 2.0).to_radians();
  // The arc's centre sits on the bisector, one radius off each face.
  let leg = r / half.tan();
  let cp = [leg, r];
  let arc = arc_pts(steps + 1, r, cp, ang + 90.0, 180.0 - ang, true);

  // Away from the sloping face, which is where its excess reaches.
  let outward = (ang + 90.0).to_radians();
  let mut path = vec![[
    arc[0][0] + excess * outward.cos(),
    arc[0][1] + excess * outward.sin(),
  ]];
  path.extend(arc.iter().copied());
  let last = *path.last().unwrap();
  path.push([last[0], -excess]);
  // The corner itself, pushed out past both faces.
  path.push([-excess / half.tan(), -excess]);
  ccw(path)
}

fn build_fillet(args: &Args) -> LuaResult<Option<ScadNode>> {
  // A rounded or chamfered end on the fillet itself is a separate shape
  // again, and still goes through OpenSCAD.
  if [
    "rounding",
    "rounding1",
    "rounding2",
    "chamfer",
    "chamfer1",
    "chamfer2",
  ]
  .iter()
  .any(|p| args.has(p))
  {
    return Ok(None);
  }
  let l = args
    .num("l")
    .or_else(|| args.num("length"))
    .or_else(|| args.num("h"))
    .or_else(|| args.num("height"));
  let Some(l) = l else {
    return args.err("l is required");
  };
  let r1 = args.radius_end("r1", "d1", "r", "d", None);
  let r2 = args.radius_end("r2", "d2", "r", "d", None);
  let (Some(r1), Some(r2)) = (r1, r2) else {
    return args.err("r is required");
  };
  let ang = args.num_or("ang", 90.0);
  if !(0.0..180.0).contains(&ang) || ang == 0.0 {
    return args.err("ang must be between 0 and 180");
  }
  // BOSL2 renamed this parameter; the old one still reads.
  let excess = args
    .num("excess")
    .or_else(|| args.num("overlap"))
    .unwrap_or(0.01);

  let steps =
    ((args.segments(r1.max(r2)) as f64 * (180.0 - ang) / 360.0).ceil() as u32)
      .max(2);
  let bottom = fillet_profile(r1, ang, excess, steps);

  let node = if (r1 - r2).abs() < EPS {
    ScadNode::LinearExtrude {
      height: l as f32,
      center: true,
      twist: 0.0,
      slices: 1,
      scale: 1.0,
      child: Box::new(crate::bosl::shapes2d::path_node(&bottom)),
    }
  } else {
    // A tapered fillet is the two ends lofted together; both profiles have
    // the same point count, so they pair up directly.
    let top = fillet_profile(r2, ang, excess, steps);
    let rows: Vec<Vec<V3>> = [(&bottom, -l / 2.0), (&top, l / 2.0)]
      .iter()
      .map(|(path, z)| path.iter().map(|p| [p[0], p[1], *z]).collect())
      .collect();
    Vnf::vertex_array(&rows, Caps::BOTH, true, false).to_node()
  };

  let leg = r1.max(r2) / (ang / 2.0).to_radians().tan();
  let attachable = Attachable::new(Geom::Prismoid {
    size: [leg, leg, l],
    size2: [leg, leg],
    shift: [0.0, 0.0],
    axis: [0.0, 0.0, 1.0],
  });
  Ok(Some(reorient(node, args, &attachable)?))
}

// ---------------------------------------------------------------------------
// Text
// ---------------------------------------------------------------------------

/// The text alignment BOSL2 derives from the anchor rather than taking
/// directly, so that `anchor = bosl.RIGHT` right-aligns the letters instead
/// of shifting an already-left-aligned block.
fn text_alignment(anchor: V3, atype: &str) -> (String, String) {
  let halign = if anchor[0] < 0.0 {
    "left"
  } else if anchor[0] > 0.0 {
    "right"
  } else {
    "center"
  };
  let valign = if anchor[1] < 0.0 {
    "bottom"
  } else if anchor[1] > 0.0 {
    "top"
  } else if atype == "baseline" {
    "baseline"
  } else {
    "center"
  };
  (halign.to_string(), valign.to_string())
}

fn build_text3d(args: &Args) -> LuaResult<Option<ScadNode>> {
  // Letter spacing, writing direction and script selection are the font
  // engine's business, and only OpenSCAD's own `text()` takes them.
  if ["spacing", "direction", "language", "script"]
    .iter()
    .any(|p| args.has(p))
  {
    return Ok(None);
  }
  let Some(text) = args.string("text") else {
    return args.err("text is required");
  };
  let h = args
    .num("h")
    .or_else(|| args.num("height"))
    .or_else(|| args.num("thickness"))
    .unwrap_or(1.0);
  let size = args.num_or("size", 10.0);
  let center = args.bool_or("center", false);
  let atype = args
    .string("atype")
    .unwrap_or_else(|| if center { "ycenter" } else { "baseline" }.to_string());
  if atype != "ycenter" && atype != "baseline" {
    return args.err("atype must be 'ycenter' or 'baseline'");
  }
  let dflt = if center { [0.0; 3] } else { [-1.0, 0.0, 0.0] };
  let anchor = match args.anchor()? {
    Some(a) => match a.as_vector() {
      Some(v) => v,
      None => return args.err("that anchor name means nothing to text3d()"),
    },
    None => dflt,
  };
  let (halign, valign) = text_alignment(anchor, &atype);

  let node = ScadNode::LinearExtrude {
    height: h as f32,
    center: true,
    twist: 0.0,
    slices: 1,
    scale: 1.0,
    child: Box::new(ScadNode::Text {
      text,
      size: size as f32,
      font: args.string("font").unwrap_or_default(),
      halign,
      valign,
    }),
  };
  // The X and Y anchors are already in the alignment, so only Z is left to
  // move the letters by.
  let attachable = Attachable::new(Geom::Prismoid {
    size: [size, size, h],
    size2: [size, size],
    shift: [0.0, 0.0],
    axis: [0.0, 0.0, 1.0],
  });
  let m = crate::bosl::attach::placement(
    &attachable,
    Some(&crate::bosl::args::Anchor::Vector([0.0, 0.0, anchor[2]])),
    args.spin(),
    args.orient(),
  );
  Ok(Some(match m {
    Some(m) => crate::bosl::attach::transform(node, m),
    None => node,
  }))
}

/// The distance along a path to each point on it.
fn cumulative_lengths(path: &[V3]) -> Vec<f64> {
  let mut out = Vec::with_capacity(path.len());
  let mut total = 0.0;
  out.push(0.0);
  for w in path.windows(2) {
    total += crate::bosl::vecmath::norm(crate::bosl::vecmath::sub(w[1], w[0]));
    out.push(total);
  }
  out
}

/// The point at a given distance along a path, with the direction it runs in.
fn point_at_length(path: &[V3], lengths: &[f64], d: f64) -> (V3, V3) {
  let last = path.len() - 1;
  let mut i = 0;
  while i < last && lengths[i + 1] < d {
    i += 1;
  }
  let span = lengths[i + 1] - lengths[i];
  let u = if span <= EPS {
    0.0
  } else {
    (d - lengths[i]) / span
  };
  let dir = crate::bosl::vecmath::sub(path[i + 1], path[i]);
  (
    crate::bosl::vecmath::lerp3(path[i], path[i + 1], u),
    crate::bosl::vecmath::unit_or(dir, [1.0, 0.0, 0.0]),
  )
}

/// A value that is either one number or one per letter.
fn per_letter(args: &Args, name: &str, n: usize) -> Option<Vec<f64>> {
  if let Some(one) = args.num(name) {
    return Some(vec![one; n]);
  }
  let mut v = args.nums(name)?;
  if v.len() == 1 {
    return Some(vec![v[0]; n]);
  }
  v.resize(n, 0.0);
  Some(v)
}

fn build_path_text(args: &Args) -> LuaResult<Option<ScadNode>> {
  let Some(text) = args.string("text") else {
    return args.err("text is required");
  };
  let letters: Vec<char> = text.chars().collect();
  if letters.is_empty() {
    return Ok(Some(ScadNode::Union(vec![])));
  }
  // Without a letter size there is nothing to space the glyphs by; OpenSCAD
  // can measure them itself, so the call goes there instead.
  let Some(lsize) = per_letter(args, "lettersize", letters.len()) else {
    return Ok(None);
  };
  let path = crate::bosl::paths::read_path(args, "path")?;
  if path.len() < 2 {
    return args.err("the path needs at least two points");
  }
  // The default `top` only matches BOSL2's curve normal while the path lies
  // flat; a path that climbs needs one given, or OpenSCAD to work it out.
  let flat = path.iter().all(|p| (p[2] - path[0][2]).abs() < EPS);
  let top = match args.vec3("top") {
    Some(v) => v,
    None if args.has("top") || args.has("normal") => return Ok(None),
    None if flat => [0.0, 0.0, 1.0],
    None => return Ok(None),
  };
  if args.string("valign").as_deref().unwrap_or("baseline") != "baseline"
    && args.num("valign").is_none()
  {
    return Ok(None);
  }

  let thickness = args
    .num("thickness")
    .or_else(|| args.num("h"))
    .or_else(|| args.num("height"))
    .unwrap_or(1.0);
  let offset = args.num_or("offset", 0.0);
  let reverse = args.bool_or("reverse", false);
  let vadjustment = -args.num_or("valign", 0.0);
  let kern = per_letter(args, "kern", letters.len().saturating_sub(1))
    .unwrap_or_else(|| vec![0.0; letters.len().saturating_sub(1)]);

  let lengths = cumulative_lengths(&path);
  let total = *lengths.last().unwrap();
  let text_length: f64 = lsize.iter().sum::<f64>() + kern.iter().sum::<f64>();
  if text_length > total + EPS {
    return args.err("the path is too short for the text");
  }
  let start = if args.bool_or("center", false) {
    (total - text_length) / 2.0
  } else {
    0.0
  };

  // Each letter sits at the middle of its own advance, so the string reads
  // evenly however the path curves.
  let mut at = start;
  let mut parts: Vec<ScadNode> = Vec::new();
  for (i, letter) in letters.iter().enumerate() {
    let centre = at + lsize[i] / 2.0;
    at += lsize[i] + kern.get(i).copied().unwrap_or(0.0);
    let (pos, tangent) = point_at_length(&path, &lengths, centre);

    // The letter's own frame: it reads along the path, stands up towards
    // `top`, and is extruded out of the plane the two of them span.
    let y = crate::bosl::vecmath::unit_or(top, [0.0, 0.0, 1.0]);
    let z = crate::bosl::vecmath::unit_or_none(crate::bosl::vecmath::cross(
      tangent, y,
    ));
    let Some(z) = z else {
      return args.err("the text's top direction runs along the path");
    };
    let z = if reverse {
      crate::bosl::vecmath::mul(z, -1.0)
    } else {
      z
    };
    let x = crate::bosl::vecmath::cross(y, z);
    let frame = Mat4([
      x[0], y[0], z[0], pos[0], //
      x[1], y[1], z[1], pos[1], //
      x[2], y[2], z[2], pos[2], //
      0.0, 0.0, 0.0, 1.0,
    ]);

    let glyph = ScadNode::LinearExtrude {
      height: thickness as f32,
      center: false,
      twist: 0.0,
      slices: 1,
      scale: 1.0,
      child: Box::new(ScadNode::Translate {
        x: 0.0,
        y: vadjustment as f32,
        z: 0.0,
        child: Box::new(ScadNode::Text {
          text: letter.to_string(),
          size: args.num_or("size", 10.0) as f32,
          font: args.string("font").unwrap_or_default(),
          halign: "center".to_string(),
          valign: "baseline".to_string(),
        }),
      }),
    };
    // The extrusion is centred on the path, offset out of it if asked.
    let placed = ScadNode::Translate {
      x: 0.0,
      y: 0.0,
      z: (offset - thickness / 2.0) as f32,
      child: Box::new(glyph),
    };
    parts.push(crate::bosl::attach::transform(placed, frame));
  }
  Ok(Some(ScadNode::Union(parts)))
}

const REGULAR_PRISM_PARAMS: &[&str] = &[
  "n",
  "h",
  "r",
  "center",
  "l",
  "length",
  "height",
  "r1",
  "r2",
  "ir",
  "ir1",
  "ir2",
  "or",
  "or1",
  "or2",
  "side",
  "side1",
  "side2",
  "d",
  "d1",
  "d2",
  "id",
  "id1",
  "id2",
  "od",
  "od1",
  "od2",
  "chamfer",
  "chamfer1",
  "chamfer2",
  "chamfang",
  "chamfang1",
  "chamfang2",
  "rounding",
  "rounding1",
  "rounding2",
  "realign",
  "shift",
  "teardrop",
  "from_end",
  "from_end1",
  "from_end2",
  "texture",
  "tex_size",
  "tex_reps",
  "tex_inset",
  "tex_rot",
  "tex_depth",
  "tex_samples",
  "tex_taper",
  "style",
];

#[cfg(test)]
mod tests {
  use super::*;
  use crate::export::materialize_scad_manifold;
  use std::f64::consts::PI;

  fn volume(node: &ScadNode) -> f64 {
    materialize_scad_manifold(node).volume()
  }

  fn bbox(node: &ScadNode) -> ([f32; 3], [f32; 3]) {
    materialize_scad_manifold(node).bounding_box()
  }

  /// `text3d` and `path_text` outline a system font, so they build nothing at
  /// all on a machine that has none. Those tests skip there rather than fail.
  fn have_fonts() -> bool {
    crate::text_render::has_system_font()
  }

  fn build(name: &str, code: &str) -> ScadNode {
    let lua = mlua::Lua::new();
    let v: mlua::Value = lua.load(code).eval().unwrap();
    let mv = mlua::MultiValue::from_iter([v]);
    let (params, f) = builder(name).expect("the shape has a native builder");
    let args =
      Args::parse(Box::leak(name.to_string().into_boxed_str()), params, &mv)
        .unwrap();
    f(&args).unwrap().expect("the arguments are supported")
  }

  #[test]
  fn a_plain_cuboid_has_the_volume_of_its_size() {
    let n = build("cuboid", "return { {30, 20, 10} }");
    assert!((volume(&n) - 6000.0).abs() < 1e-3);
  }

  #[test]
  fn a_rounded_cuboid_has_the_volume_the_fillets_leave() {
    let r: f64 = 4.0;
    let n = build(
      "cuboid",
      "return { {20, 20, 20}, rounding = 4, ['fn'] = 256 }",
    );
    // Each of the twelve edges loses the sliver outside its quarter-round,
    // and each corner cube keeps only its sphere octant.
    let edge_loss = 12.0 * (r * r - PI * r * r / 4.0) * (20.0 - 2.0 * r);
    let corner_loss = 8.0 * (r.powi(3) - PI * r.powi(3) / 6.0);
    let v = volume(&n);
    assert!((v - (8000.0 - edge_loss - corner_loss)).abs() < 2.0, "{v}");
  }

  #[test]
  fn rounding_a_cube_by_half_its_side_makes_a_sphere() {
    let v = volume(&build(
      "cuboid",
      "return { {10, 10, 10}, rounding = 5, ['fn'] = 256 }",
    ));
    let sphere = 4.0 / 3.0 * PI * 125.0;
    assert!((v / sphere - 1.0).abs() < 1e-3, "{v} vs {sphere}");
  }

  #[test]
  fn a_chamfered_cube_cuts_its_corners_off() {
    let c: f64 = 3.0;
    let n = build("cuboid", "return { {20, 20, 20}, chamfer = 3 }");
    // Twelve edge prisms come off, and each corner cube of side c keeps
    // only the tetrahedron beyond the plane through the three cuts.
    let edge_loss = 12.0 * (c * c / 2.0) * (20.0 - 2.0 * c);
    let corner_loss = 8.0 * (c.powi(3) - c.powi(3) / 6.0);
    let v = volume(&n);
    assert!((v - (8000.0 - edge_loss - corner_loss)).abs() < 1e-3, "{v}");
  }

  #[test]
  fn rounding_only_the_vertical_edges_leaves_the_others_sharp() {
    let r: f64 = 4.0;
    let n = build(
      "cuboid",
      "return { {20, 20, 20}, rounding = 4, edges = 'Z', ['fn'] = 256 }",
    );
    // Only the four vertical edges lose anything, and they lose it over the
    // full height because the horizontal edges stay square.
    let lost = 4.0 * (r * r - PI * r * r / 4.0) * 20.0;
    let v = volume(&n);
    assert!((v - (8000.0 - lost)).abs() < 1.0, "{v}");
  }

  #[test]
  fn a_rounding_too_large_for_the_box_is_rejected() {
    let lua = mlua::Lua::new();
    let v: mlua::Value = lua
      .load("return { {10, 10, 10}, rounding = 6 }")
      .eval()
      .unwrap();
    let mv = mlua::MultiValue::from_iter([v]);
    let (params, f) = builder("cuboid").unwrap();
    let args = Args::parse("cuboid", params, &mv).unwrap();
    assert!(f(&args).is_err());
  }

  #[test]
  fn a_cylinder_has_the_volume_its_radius_implies() {
    let n = build("cyl", "return { h = 10, r = 5, ['fn'] = 256 }");
    let ideal = PI * 25.0 * 10.0;
    assert!((volume(&n) / ideal - 1.0).abs() < 1e-3);
  }

  #[test]
  fn a_cone_is_a_third_of_its_cylinder() {
    let n = build("cyl", "return { h = 12, r1 = 6, r2 = 0, ['fn'] = 256 }");
    let ideal = PI * 36.0 * 12.0 / 3.0;
    assert!((volume(&n) / ideal - 1.0).abs() < 1e-3);
  }

  #[test]
  fn a_cylinder_centres_on_the_origin_by_default() {
    let n = build("cyl", "return { h = 10, r = 5 }");
    let (lo, hi) = bbox(&n);
    assert!((lo[2] + 5.0).abs() < 1e-3, "{lo:?}");
    assert!((hi[2] - 5.0).abs() < 1e-3, "{hi:?}");
  }

  #[test]
  fn anchoring_a_cylinder_at_its_base_puts_it_on_the_xy_plane() {
    let n = build("cyl", "return { h = 10, r = 5, anchor = {0, 0, -1} }");
    let (lo, hi) = bbox(&n);
    assert!(lo[2].abs() < 1e-3, "{lo:?}");
    assert!((hi[2] - 10.0).abs() < 1e-3, "{hi:?}");
  }

  #[test]
  fn an_x_cylinder_lies_along_the_x_axis() {
    let n = build("xcyl", "return { h = 20, r = 3, ['fn'] = 128 }");
    let (lo, hi) = bbox(&n);
    assert!((hi[0] - 10.0).abs() < 1e-3, "{hi:?}");
    assert!((lo[0] + 10.0).abs() < 1e-3, "{lo:?}");
    assert!((hi[1] - 3.0).abs() < 0.01, "{hi:?}");
  }

  #[test]
  fn rounding_a_cylinder_end_removes_material() {
    let plain =
      volume(&build("cyl", "return { h = 20, r = 10, ['fn'] = 128 }"));
    let round = volume(&build(
      "cyl",
      "return { h = 20, r = 10, rounding = 3, ['fn'] = 128 }",
    ));
    assert!(round < plain, "{round} vs {plain}");
    // Each end loses the corner ring left outside a quarter-round of r = 3.
    let lost =
      2.0 * (3.0f64.powi(2) - PI * 9.0 / 4.0) * 2.0 * PI * (10.0 - 1.5);
    assert!(
      (plain - round - lost).abs() / lost < 0.1,
      "{}",
      plain - round
    );
  }

  #[test]
  fn chamfering_a_cylinder_end_removes_a_ring() {
    let plain =
      volume(&build("cyl", "return { h = 20, r = 10, ['fn'] = 256 }"));
    let cham = volume(&build(
      "cyl",
      "return { h = 20, r = 10, chamfer = 2, ['fn'] = 256 }",
    ));
    assert!(cham < plain);
  }

  #[test]
  fn a_tube_is_the_difference_of_its_two_cylinders() {
    let n = build(
      "tube",
      "return { h = 10, ['or'] = 10, ir = 6, ['fn'] = 256 }",
    );
    let ideal = PI * (100.0 - 36.0) * 10.0;
    assert!((volume(&n) / ideal - 1.0).abs() < 1e-2, "{}", volume(&n));
  }

  #[test]
  fn a_tube_takes_its_bore_from_the_wall_thickness() {
    let a = volume(&build(
      "tube",
      "return { h = 10, ['or'] = 10, wall = 4, ['fn'] = 256 }",
    ));
    let b = volume(&build(
      "tube",
      "return { h = 10, ['or'] = 10, ir = 6, ['fn'] = 256 }",
    ));
    assert!((a - b).abs() < 1e-3, "{a} vs {b}");
  }

  /// `tube()` straddles the XY plane by default, and only `center = false`
  /// stands it on the plane — the two are different anchors in BOSL2.
  #[test]
  fn a_tube_centres_by_default_and_center_false_stands_it_up() {
    let n = build("tube", "return { h = 10, ['or'] = 10, ir = 6 }");
    let (lo, hi) = bbox(&n);
    assert!((lo[2] + 5.0).abs() < 1e-3, "{lo:?}");
    assert!((hi[2] - 5.0).abs() < 1e-3, "{hi:?}");

    let n = build(
      "tube",
      "return { h = 10, ['or'] = 10, ir = 6, center = false }",
    );
    let (lo, hi) = bbox(&n);
    assert!(lo[2].abs() < 1e-3, "{lo:?}");
    assert!((hi[2] - 10.0).abs() < 1e-3, "{hi:?}");
  }

  #[test]
  fn a_sphere_has_the_volume_its_radius_implies() {
    let n = build("spheroid", "return { r = 10, ['fn'] = 128 }");
    let ideal = 4.0 / 3.0 * PI * 1000.0;
    assert!((volume(&n) / ideal - 1.0).abs() < 1e-2, "{}", volume(&n));
  }

  #[test]
  fn a_torus_has_the_volume_pappus_gives_it() {
    let n = build("torus", "return { r_maj = 20, r_min = 5, ['fn'] = 128 }");
    let ideal = 2.0 * PI * PI * 20.0 * 25.0;
    assert!((volume(&n) / ideal - 1.0).abs() < 2e-2, "{}", volume(&n));
  }

  #[test]
  fn a_torus_can_be_given_by_its_inner_and_outer_radius() {
    let a = volume(&build(
      "torus",
      "return { ['or'] = 25, ir = 15, ['fn'] = 128 }",
    ));
    let b = volume(&build(
      "torus",
      "return { r_maj = 20, r_min = 5, ['fn'] = 128 }",
    ));
    assert!((a - b).abs() / b < 1e-6, "{a} vs {b}");
  }

  #[test]
  fn a_prismoid_tapers_between_its_two_sizes() {
    let n = build(
      "prismoid",
      "return { size1 = {40, 40}, size2 = {20, 20}, h = 30 }",
    );
    // The volume of a frustum with square ends.
    let ideal = 30.0 / 3.0 * (1600.0 + 400.0 + (1600.0f64 * 400.0).sqrt());
    assert!((volume(&n) - ideal).abs() < 1.0, "{}", volume(&n));
  }

  #[test]
  fn a_prismoid_sits_on_the_xy_plane_by_default() {
    let n = build(
      "prismoid",
      "return { size1 = {40, 40}, size2 = {20, 20}, h = 30 }",
    );
    let (lo, hi) = bbox(&n);
    assert!(lo[2].abs() < 1e-3, "{lo:?}");
    assert!((hi[2] - 30.0).abs() < 1e-3, "{hi:?}");
  }

  #[test]
  fn a_rect_tube_leaves_the_wall_it_is_given() {
    let n = build("rect_tube", "return { size = {40, 40}, wall = 5, h = 30 }");
    let ideal = (40.0 * 40.0 - 30.0 * 30.0) * 30.0;
    assert!((volume(&n) - ideal).abs() < 1.0, "{}", volume(&n));
  }

  #[test]
  fn a_wedge_is_half_its_bounding_box() {
    let n = build("wedge", "return { {30, 30, 20} }");
    assert!((volume(&n) - 30.0 * 30.0 * 20.0 / 2.0).abs() < 1e-3);
  }

  #[test]
  fn a_wedge_sits_in_the_positive_octant_by_default() {
    let n = build("wedge", "return { {30, 30, 20} }");
    let (lo, _) = bbox(&n);
    assert!(lo[0].abs() < 1e-3 && lo[1].abs() < 1e-3 && lo[2].abs() < 1e-3);
  }

  #[test]
  fn an_octahedron_is_a_sixth_of_its_bounding_box() {
    let n = build("octahedron", "return { size = 30 }");
    assert!((volume(&n) - 30.0f64.powi(3) / 6.0).abs() < 1e-3);
  }

  #[test]
  fn a_pie_slice_is_its_fraction_of_the_cylinder() {
    let n = build("pie_slice", "return { r = 25, h = 20, ang = 90 }");
    let ideal = PI * 625.0 * 20.0 / 4.0;
    assert!((volume(&n) / ideal - 1.0).abs() < 0.02, "{}", volume(&n));
  }

  #[test]
  fn a_reflex_pie_slice_covers_more_than_half_the_cylinder() {
    let n = build("pie_slice", "return { r = 25, h = 20, ang = 270 }");
    let ideal = PI * 625.0 * 20.0 * 0.75;
    assert!((volume(&n) / ideal - 1.0).abs() < 0.02, "{}", volume(&n));
  }

  #[test]
  fn a_regular_prism_has_the_area_of_its_polygon() {
    let n = build("regular_prism", "return { n = 6, r = 20, h = 30 }");
    let ideal = 6.0 * 0.5 * 400.0 * (60f64.to_radians().sin()) * 30.0;
    assert!((volume(&n) - ideal).abs() < 1.0, "{}", volume(&n));
  }

  #[test]
  fn an_onion_is_larger_than_the_sphere_it_caps() {
    let n = build("onion", "return { r = 15, ['fn'] = 128 }");
    let sphere = 4.0 / 3.0 * PI * 15f64.powi(3);
    let v = volume(&n);
    assert!(v > sphere * 0.99, "{v} vs {sphere}");
    let (_, hi) = bbox(&n);
    assert!(hi[2] > 15.0, "{hi:?}");
  }

  #[test]
  fn a_teardrop_lies_along_the_y_axis() {
    let n = build("teardrop", "return { r = 15, h = 20 }");
    let (lo, hi) = bbox(&n);
    assert!((hi[1] - 10.0).abs() < 1e-3, "{hi:?}");
    assert!((lo[1] + 10.0).abs() < 1e-3, "{lo:?}");
    // The cap makes it taller than the circle it is built from.
    assert!(hi[2] > 15.0, "{hi:?}");
  }

  fn maybe_build(name: &'static str, code: &str) -> Option<ScadNode> {
    let lua = mlua::Lua::new();
    let v: mlua::Value = lua.load(code).eval().unwrap();
    let mv = mlua::MultiValue::from_iter([v]);
    let (params, f) = builder(name).expect("the shape has a native builder");
    f(&Args::parse(name, params, &mv).unwrap()).unwrap()
  }

  #[test]
  fn a_fillet_fills_the_corner_two_faces_leave() {
    let n = build("fillet", "return { l = 10, r = 3, excess = 0, fn = 128 }");
    // The corner square, less the quarter circle the arc cuts out of it.
    let ideal = (9.0 - PI * 9.0 / 4.0) * 10.0;
    assert!((volume(&n) - ideal).abs() / ideal < 0.01, "{}", volume(&n));
    let (lo, hi) = bbox(&n);
    // It sits in the corner, reaching one leg along each face.
    assert!(lo[0].abs() < 1e-3 && lo[1].abs() < 1e-3, "{lo:?}");
    assert!((hi[0] - 3.0).abs() < 1e-3, "{hi:?}");
    assert!((hi[2] - 5.0).abs() < 1e-3, "{hi:?}");
  }

  #[test]
  fn a_sharper_fillet_holds_more_material() {
    let square = volume(&build(
      "fillet",
      "return { l = 10, r = 3, ang = 90, excess = 0, fn = 128 }",
    ));
    let sharp = volume(&build(
      "fillet",
      "return { l = 10, r = 3, ang = 60, excess = 0, fn = 128 }",
    ));
    // A sharper corner reaches further along both faces.
    assert!(sharp > square, "{sharp} against {square}");
  }

  #[test]
  fn a_tapered_fillet_lofts_between_its_two_radii() {
    let n = build(
      "fillet",
      "return { l = 10, r1 = 2, r2 = 4, excess = 0, fn = 128 }",
    );
    let ends = [2.0f64, 4.0].map(|r| (r * r - PI * r * r / 4.0) * 10.0);
    let v = volume(&n);
    assert!(v > ends[0] && v < ends[1], "{v} outside {ends:?}");
  }

  #[test]
  fn a_fillet_with_an_end_treatment_falls_back_to_openscad() {
    assert!(
      maybe_build("fillet", "return { l = 10, r = 3, rounding = 1 }").is_none()
    );
  }

  #[test]
  fn text3d_stands_the_letters_up_as_a_solid() {
    if !have_fonts() {
      return;
    }
    let n = build("text3d", "return { 'Hi', size = 10, h = 2 }");
    let (lo, hi) = bbox(&n);
    assert!(volume(&n) > 0.0);
    assert!((hi[2] - lo[2] - 2.0).abs() < 1e-3, "{lo:?} {hi:?}");
    // The default anchor is the left of the baseline, so the letters run
    // right from the origin and sit on it, give or take the side bearing.
    assert!(lo[0] >= 0.0 && lo[0] < 2.0, "{lo:?}");
    assert!(lo[1].abs() < 0.5, "{lo:?}");
  }

  #[test]
  fn text3d_reads_its_alignment_off_the_anchor() {
    if !have_fonts() {
      return;
    }
    let (_, right) = bbox(&build(
      "text3d",
      "return { 'Hi', size = 10, h = 2, anchor = { 1, 0, 0 } }",
    ));
    // Anchored right, the letters end at the origin instead of starting.
    assert!(right[0] < 0.5, "{right:?}");
  }

  #[test]
  fn text3d_with_font_options_falls_back_to_openscad() {
    assert!(maybe_build("text3d", "return { 'Hi', spacing = 1.5 }").is_none());
  }

  #[test]
  fn path_text_sets_the_letters_along_the_path() {
    if !have_fonts() {
      return;
    }
    let n = build(
      "path_text",
      "return { path = { {0,0,0}, {60,0,0} }, text = 'ABC',
                size = 10, lettersize = 10, thickness = 2 }",
    );
    let (lo, hi) = bbox(&n);
    assert!(volume(&n) > 0.0);
    // Three 10 mm letters laid end to end, centred on their own advances.
    assert!(lo[0] > -1.0 && lo[0] < 5.0, "{lo:?}");
    assert!(hi[0] > 20.0 && hi[0] < 30.0, "{lo:?} {hi:?}");
    // They stand up in Z and are extruded across the path.
    assert!(hi[2] > 5.0, "{hi:?}");
    assert!((hi[1] - lo[1] - 2.0).abs() < 1e-3, "{lo:?} {hi:?}");
  }

  #[test]
  fn path_text_bends_the_letters_round_a_curve() {
    if !have_fonts() {
      return;
    }
    let straight = bbox(&build(
      "path_text",
      "return { path = { {0,0,0}, {60,0,0} }, text = 'ABC',
                size = 10, lettersize = 10 }",
    ));
    let curved = bbox(&build(
      "path_text",
      "return { path = { {0,0,0}, {20,20,0}, {40,0,0} }, text = 'ABC',
                size = 10, lettersize = 10 }",
    ));
    // The middle letter climbs with the path instead of staying on the axis.
    assert!(curved.1[1] > straight.1[1] + 5.0, "{curved:?}");
  }

  #[test]
  fn path_text_without_a_letter_size_falls_back_to_openscad() {
    assert!(
      maybe_build(
        "path_text",
        "return { path = { {0,0,0}, {60,0,0} }, text = 'ABC', size = 10 }",
      )
      .is_none()
    );
  }

  #[test]
  fn path_text_on_a_path_that_climbs_falls_back_to_openscad() {
    // Without a `top` or `normal` the letters have nothing to stand up
    // against once the path leaves the plane.
    assert!(
      maybe_build(
        "path_text",
        "return { path = { {0,0,0}, {30,0,20} }, text = 'A',
                  size = 10, lettersize = 10 }",
      )
      .is_none()
    );
  }

  #[test]
  fn path_text_refuses_a_string_longer_than_its_path() {
    let lua = mlua::Lua::new();
    let v: mlua::Value = lua
      .load(
        "return { path = { {0,0,0}, {5,0,0} }, text = 'ABC',
                  size = 10, lettersize = 10 }",
      )
      .eval()
      .unwrap();
    let mv = mlua::MultiValue::from_iter([v]);
    let (params, f) = builder("path_text").unwrap();
    let err = f(&Args::parse("path_text", params, &mv).unwrap())
      .unwrap_err()
      .to_string();
    assert!(err.contains("too short"), "{err}");
  }

  #[test]
  fn a_textured_cylinder_falls_back_to_openscad() {
    let lua = mlua::Lua::new();
    let v: mlua::Value = lua
      .load("return { h = 10, r = 5, texture = 'diamonds' }")
      .eval()
      .unwrap();
    let mv = mlua::MultiValue::from_iter([v]);
    let (params, f) = builder("cyl").unwrap();
    let args = Args::parse("cyl", params, &mv).unwrap();
    assert!(f(&args).unwrap().is_none());
  }
}
