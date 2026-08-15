//! BOSL2's `rounding.scad`: the end-treatment specifications.
//!
//! `offset_sweep()`, `convex_offset_extrude()` and `offset_stroke()` all
//! finish the ends of what they sweep, and they all describe that finish the
//! same way — with a spec built by one of the `os_*` constructors. The spec
//! is a plain table of settings; turning it into geometry is
//! [`rounding_offsets`], which returns the profile as a list of
//! `[horizontal, rise]` steps away from the outline.
//!
//! A positive radius pulls the outline in as it rises, which rounds the end
//! over. A negative one pushes it out, which flares the end into a fillet
//! against whatever the sweep is standing on.

use mlua::{Lua, Result as LuaResult, Value as LuaValue};

use crate::bosl::args::as_num;
use crate::bosl::value::{Args, Val};

const EPS: f64 = 1e-9;

/// How the end of a swept outline is finished.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EdgeType {
  Circle,
  Teardrop,
  Chamfer,
  Smooth,
  Profile,
}

impl EdgeType {
  fn parse(name: &str) -> Option<EdgeType> {
    match name {
      "circle" => Some(EdgeType::Circle),
      "teardrop" => Some(EdgeType::Teardrop),
      "chamfer" => Some(EdgeType::Chamfer),
      "smooth" => Some(EdgeType::Smooth),
      "profile" => Some(EdgeType::Profile),
      _ => None,
    }
  }
}

/// Everything an end treatment needs to draw its profile.
///
/// The fields mirror BOSL2's struct keys one for one, so a spec written by
/// `os_circle` and one written by hand read the same way here.
#[derive(Clone, Debug)]
pub struct EdgeSpec {
  pub edge_type: EdgeType,
  pub r: Option<f64>,
  pub cut: Option<f64>,
  pub joint: Option<f64>,
  pub k: f64,
  pub angle: f64,
  pub chamfer_width: Option<f64>,
  pub chamfer_height: Option<f64>,
  pub points: Vec<[f64; 2]>,
  pub extra: f64,
  pub steps: u32,
  pub offset: String,
  pub check_valid: bool,
  pub quality: f64,
}

impl Default for EdgeSpec {
  fn default() -> EdgeSpec {
    EdgeSpec {
      edge_type: EdgeType::Circle,
      r: None,
      cut: None,
      joint: None,
      k: 0.75,
      angle: 45.0,
      chamfer_width: None,
      chamfer_height: None,
      points: Vec::new(),
      extra: 0.0,
      steps: 16,
      offset: "round".to_string(),
      check_valid: true,
      quality: 1.0,
    }
  }
}

/// Read a numeric field from a spec table.
fn spec_num(t: &mlua::Table, key: &str) -> Option<f64> {
  t.get::<LuaValue>(key).ok().as_ref().and_then(as_num)
}

fn spec_string(t: &mlua::Table, key: &str) -> Option<String> {
  match t.get::<LuaValue>(key).ok()? {
    LuaValue::String(s) => s.to_str().ok().map(|s| s.to_string()),
    _ => None,
  }
}

fn spec_bool(t: &mlua::Table, key: &str) -> Option<bool> {
  match t.get::<LuaValue>(key).ok()? {
    LuaValue::Boolean(b) => Some(b),
    LuaValue::Number(n) => Some(n != 0.0),
    LuaValue::Integer(n) => Some(n != 0),
    _ => None,
  }
}

fn spec_points(t: &mlua::Table, key: &str) -> Option<Vec<[f64; 2]>> {
  let LuaValue::Table(list) = t.get::<LuaValue>(key).ok()? else {
    return None;
  };
  let mut out = Vec::new();
  for i in 1..=list.raw_len() {
    let v = crate::bosl::args::as_nums(&list.get::<LuaValue>(i).ok()?)?;
    if v.len() < 2 {
      return None;
    }
    out.push([v[0], v[1]]);
  }
  Some(out)
}

impl EdgeSpec {
  /// Read a spec table, filling anything it leaves out from `defaults`.
  ///
  /// The defaults carry the sweep's own top-level `r`, `steps`, `offset` and
  /// friends, so `offset_sweep { r = 2, top = bosl.os_chamfer { … } }` gives
  /// the chamfered top the sweep's step count without restating it.
  pub fn from_table(t: &mlua::Table, defaults: &EdgeSpec) -> Option<EdgeSpec> {
    let edge_type = match spec_string(t, "type") {
      Some(name) => EdgeType::parse(&name)?,
      None => defaults.edge_type,
    };
    Some(EdgeSpec {
      edge_type,
      r: spec_num(t, "r").or(defaults.r),
      cut: spec_num(t, "cut").or(defaults.cut),
      joint: spec_num(t, "joint").or(defaults.joint),
      k: spec_num(t, "k").unwrap_or(defaults.k),
      angle: spec_num(t, "angle").unwrap_or(defaults.angle),
      chamfer_width: spec_num(t, "chamfer_width").or(defaults.chamfer_width),
      chamfer_height: spec_num(t, "chamfer_height").or(defaults.chamfer_height),
      points: spec_points(t, "points")
        .unwrap_or_else(|| defaults.points.clone()),
      extra: spec_num(t, "extra").unwrap_or(defaults.extra),
      steps: spec_num(t, "steps")
        .map(|n| n as u32)
        .unwrap_or(defaults.steps),
      offset: spec_string(t, "offset")
        .unwrap_or_else(|| defaults.offset.clone()),
      check_valid: spec_bool(t, "check_valid").unwrap_or(defaults.check_valid),
      quality: spec_num(t, "quality").unwrap_or(defaults.quality),
    })
  }

  /// The radius a rounded or chamfered end works from.
  ///
  /// `cut` measures the treatment differently — as the depth taken out of the
  /// corner rather than the radius of the arc — so it converts here.
  fn radius(&self) -> Option<f64> {
    match self.edge_type {
      EdgeType::Circle | EdgeType::Teardrop => match self.cut {
        Some(cut) => Some(cut / (std::f64::consts::SQRT_2 - 1.0)),
        None => self.r,
      },
      EdgeType::Chamfer => match self.cut {
        Some(cut) => Some(std::f64::consts::SQRT_2 * cut),
        None => self.r,
      },
      _ => None,
    }
  }

  /// The chamfer's run and rise, from whichever pair of measurements was
  /// given: the cut depth and an angle, the two lengths outright, or one
  /// length and an angle.
  fn chamfer_legs(&self) -> (Option<f64>, Option<f64>) {
    let ang = self.angle.to_radians();
    let width = self
      .cut
      .map(|cut| cut / ang.cos())
      .or(self.chamfer_width)
      .or_else(|| self.chamfer_height.map(|h| h * ang.tan()));
    let height = self
      .cut
      .map(|cut| cut / ang.sin())
      .or(self.chamfer_height)
      .or_else(|| self.chamfer_width.map(|w| w / ang.tan()));
    (width, height)
  }

  /// The joint length of a continuous-curvature roundover.
  fn smooth_joint(&self) -> Option<f64> {
    self.joint.or_else(|| {
      self
        .cut
        .map(|cut| 16.0 * cut / std::f64::consts::SQRT_2 / (1.0 + 4.0 * self.k))
    })
  }
}

/// Round to the nearest multiple of `q`, as BOSL2's `quant()` does.
fn quant(v: f64, q: f64) -> f64 {
  (v / q).round() * q
}

/// The control points of a continuous-curvature corner.
fn smooth_bez_fill(p: [[f64; 2]; 3], k: f64) -> [[f64; 2]; 5] {
  let lerp = |a: [f64; 2], b: [f64; 2], u: f64| {
    [a[0] + (b[0] - a[0]) * u, a[1] + (b[1] - a[1]) * u]
  };
  [p[0], lerp(p[1], p[0], k), p[1], lerp(p[1], p[2], k), p[2]]
}

/// Evaluate a Bézier of any degree at `u`, by repeated linear interpolation.
fn bezier_at2(ctrl: &[[f64; 2]], u: f64) -> [f64; 2] {
  let mut pts = ctrl.to_vec();
  while pts.len() > 1 {
    pts = pts
      .windows(2)
      .map(|w| {
        [
          w[0][0] + (w[1][0] - w[0][0]) * u,
          w[0][1] + (w[1][1] - w[0][1]) * u,
        ]
      })
      .collect();
  }
  pts[0]
}

/// The profile of an end treatment, as steps of `[horizontal, rise]` away
/// from the outline.
///
/// `z_dir` is `1` for the top of a sweep and `-1` for the bottom, so the same
/// spec builds a mirror-image profile at each end. A positive radius makes
/// the horizontal component negative — the outline draws in as it rises —
/// and a negative radius flares it out instead.
pub fn rounding_offsets(
  spec: &EdgeSpec,
  z_dir: f64,
) -> Result<Vec<[f64; 2]>, String> {
  let n = spec.steps.max(1);
  let mut offsets: Vec<[f64; 2]> = match spec.edge_type {
    EdgeType::Profile => {
      if spec
        .points
        .first()
        .map(|p| p[0].abs() < EPS && p[1].abs() < EPS)
        != Some(true)
      {
        return Err("a profile must start at [0, 0]".to_string());
      }
      spec.points[1..]
        .iter()
        .map(|p| [-p[0], z_dir * p[1]])
        .collect()
    }
    EdgeType::Chamfer => {
      let (width, height) = spec.chamfer_legs();
      if !(spec.angle > 0.0 && spec.angle < 90.0) {
        return Err(format!(
          "a chamfer's angle must be between 0 and 90, not {}",
          spec.angle
        ));
      }
      let (Some(width), Some(height)) = (width, height) else {
        return Err(
          "a chamfer needs `cut`, or one or both of `chamfer_width` and \
           `chamfer_height`"
            .to_string(),
        );
      };
      if width.abs() < EPS && height.abs() < EPS {
        Vec::new()
      } else {
        vec![[-width, z_dir * height.abs()]]
      }
    }
    EdgeType::Teardrop => {
      let radius = spec
        .radius()
        .ok_or_else(|| "a teardrop needs `r` or `cut`".to_string())?;
      if radius.abs() < EPS {
        Vec::new()
      } else {
        let mut out: Vec<[f64; 2]> = (1..=n)
          .map(|i| {
            let a = (i as f64 * 45.0 / n as f64).to_radians();
            [radius * (a.cos() - 1.0), z_dir * radius.abs() * a.sin()]
          })
          .collect();
        out.push([
          -2.0 * radius * (1.0 - std::f64::consts::SQRT_2 / 2.0),
          z_dir * radius.abs(),
        ]);
        out
      }
    }
    EdgeType::Circle => {
      let radius = spec
        .radius()
        .ok_or_else(|| "a roundover needs `r` or `cut`".to_string())?;
      if radius.abs() < EPS {
        Vec::new()
      } else {
        (1..=n)
          .map(|i| {
            let a = (i as f64 * 90.0 / n as f64).to_radians();
            [radius * (a.cos() - 1.0), z_dir * radius.abs() * a.sin()]
          })
          .collect()
      }
    }
    EdgeType::Smooth => {
      let joint = spec.smooth_joint().ok_or_else(|| {
        "a smooth roundover needs `joint` or `cut`".to_string()
      })?;
      if joint.abs() < EPS {
        Vec::new()
      } else {
        let corner = [
          [0.0, 0.0],
          [0.0, z_dir * joint.abs()],
          [-joint, z_dir * joint.abs()],
        ];
        let ctrl = smooth_bez_fill(corner, spec.k);
        // BOSL2 draws this corner with `$fn` set to two above the step
        // count, and drops the point sitting on the outline itself.
        let count = (n + 2).max(3);
        (1..=count)
          .map(|i| bezier_at2(&ctrl, i as f64 / count as f64))
          .collect()
      }
    }
  };

  // `extra` carries the profile past its own end, so the sweep always
  // overlaps whatever it is joining rather than meeting it exactly.
  if spec.extra > 0.0 && !offsets.is_empty() {
    let last = *offsets.last().unwrap();
    offsets.push([last[0], last[1] + z_dir * spec.extra]);
  }
  Ok(
    offsets
      .into_iter()
      .map(|p| [quant(p[0], 1.0 / 1024.0), quant(p[1], 1.0 / 1024.0)])
      .collect(),
  )
}

/// Read a sweep's own parameters as the defaults its end specs inherit.
pub fn defaults_from_args(a: &Args) -> EdgeSpec {
  EdgeSpec {
    edge_type: EdgeType::Circle,
    // An end nothing was said about is a square one, so the default radius
    // is zero rather than missing.
    r: Some(a.num_or("r", 0.0)),
    cut: a.num("cut"),
    joint: a.num("joint"),
    k: a.num_or("k", 0.75),
    angle: a.num_or("angle", 45.0),
    chamfer_width: a.num("chamfer_width"),
    chamfer_height: a.num("chamfer_height"),
    points: Vec::new(),
    extra: a.num_or("extra", 0.0),
    steps: a.num("steps").map(|n| n as u32).unwrap_or(16),
    offset: a.string("offset").unwrap_or_else(|| "round".to_string()),
    check_valid: a.bool_or("check_valid", true),
    quality: a.num_or("quality", 1.0),
  }
}

/// Read one end's spec from a sweep's arguments.
///
/// `ends` sets both ends at once, and the per-end name wins over it.
pub fn end_spec(
  a: &Args,
  names: &[&str],
  defaults: &EdgeSpec,
) -> LuaResult<EdgeSpec> {
  for name in names {
    let Some(raw) = a.raw(name) else { continue };
    let LuaValue::Table(t) = raw else {
      return a.err(format!(
        "{name} must be an end treatment such as bosl.os_circle {{ r = 2 }}"
      ));
    };
    return match EdgeSpec::from_table(t, defaults) {
      Some(spec) => Ok(spec),
      None => a.err(format!(
        "{name} is not a valid end treatment; build one with bosl.os_circle, \
         os_chamfer, os_smooth, os_teardrop, os_profile or os_mask"
      )),
    };
  }
  Ok(defaults.clone())
}

// ---------------------------------------------------------------------------
// The os_* constructors
// ---------------------------------------------------------------------------

/// Build the spec table an `os_*` constructor hands back.
fn os_table(
  lua: &Lua,
  target: &str,
  edge_type: &str,
  fields: &[(&str, Option<f64>)],
) -> LuaResult<LuaValue> {
  let t = lua.create_table()?;
  t.set("for", target)?;
  t.set("type", edge_type)?;
  for (key, value) in fields {
    if let Some(v) = value {
      t.set(*key, *v)?;
    }
  }
  Ok(LuaValue::Table(t))
}

/// Every `os_*` constructor takes the same tail of shared settings.
fn shared_fields(a: &Args) -> Vec<(&'static str, Option<f64>)> {
  vec![
    ("extra", a.num("extra")),
    ("quality", a.num("quality")),
    ("steps", a.num("steps")),
  ]
}

/// Copy the settings that are not numbers across to the spec table.
fn set_shared_rest(a: &Args, v: &LuaValue) -> LuaResult<()> {
  let LuaValue::Table(t) = v else {
    return Ok(());
  };
  if let Some(b) = a.bool("check_valid") {
    t.set("check_valid", b)?;
  }
  if let Some(s) = a.string("offset") {
    t.set("offset", s)?;
  }
  Ok(())
}

fn exactly_one(a: &Args, names: &[&str]) -> LuaResult<()> {
  let count = names.iter().filter(|n| a.num(n).is_some()).count();
  if count == 1 {
    return Ok(());
  }
  a.err(format!(
    "define exactly one of {}",
    names
      .iter()
      .map(|n| format!("`{n}`"))
      .collect::<Vec<_>>()
      .join(" and ")
  ))
}

fn os_circle(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  exactly_one(a, &["r", "cut"])?;
  let mut fields = vec![("r", a.num("r")), ("cut", a.num("cut"))];
  fields.extend(shared_fields(a));
  let v = os_table(lua, "offset_sweep", "circle", &fields)?;
  set_shared_rest(a, &v)?;
  Ok(v)
}

fn os_teardrop(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  exactly_one(a, &["r", "cut"])?;
  let mut fields = vec![("r", a.num("r")), ("cut", a.num("cut"))];
  fields.extend(shared_fields(a));
  let v = os_table(lua, "offset_sweep", "teardrop", &fields)?;
  set_shared_rest(a, &v)?;
  Ok(v)
}

fn os_chamfer(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let sized = a.num("height").is_some() || a.num("width").is_some();
  if !sized && a.num("cut").is_none() {
    return a.err("define `cut`, or one or both of `width` and `height`");
  }
  let mut fields = vec![
    ("chamfer_width", a.num("width")),
    ("chamfer_height", a.num("height")),
    ("cut", a.num("cut")),
    ("angle", a.num("angle")),
  ];
  fields.extend(shared_fields(a));
  let v = os_table(lua, "offset_sweep", "chamfer", &fields)?;
  set_shared_rest(a, &v)?;
  Ok(v)
}

fn os_smooth(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  exactly_one(a, &["joint", "cut"])?;
  let mut fields = vec![
    ("joint", a.num("joint")),
    ("k", a.num("k")),
    ("cut", a.num("cut")),
  ];
  fields.extend(shared_fields(a));
  let v = os_table(lua, "offset_sweep", "smooth", &fields)?;
  set_shared_rest(a, &v)?;
  Ok(v)
}

/// Read the point list an `os_profile` or `os_mask` call was given.
fn profile_points(a: &Args, name: &str) -> LuaResult<Vec<[f64; 2]>> {
  match a.points2(name) {
    Some(p) if p.len() >= 2 => Ok(p),
    _ => a.err(format!("{name} must be a list of 2D points")),
  }
}

fn write_points(
  lua: &Lua,
  t: &mlua::Table,
  points: &[[f64; 2]],
) -> LuaResult<()> {
  let list = lua.create_table()?;
  for (i, p) in points.iter().enumerate() {
    list.set(i + 1, Val::vec([p[0], p[1]]).to_lua(lua)?)?;
  }
  t.set("points", list)?;
  Ok(())
}

fn os_profile(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let points = profile_points(a, "points")?;
  if points[0][0].abs() > EPS || points[0][1].abs() > EPS {
    return a.err("a profile must start at [0, 0]");
  }
  let mut fields =
    vec![("extra", a.num("extra")), ("quality", a.num("quality"))];
  fields.retain(|(_, v)| v.is_some());
  let v = os_table(lua, "offset_sweep", "profile", &fields)?;
  if let LuaValue::Table(t) = &v {
    write_points(lua, t, &points)?;
  }
  set_shared_rest(a, &v)?;
  Ok(v)
}

/// Turn a 2D mask outline into the profile that cuts the same shape.
///
/// The mask is drawn as it sits against the corner, with its origin the one
/// vertex behind both faces. Rotating the outline to start there and folding
/// it into the first quadrant gives the offsets the sweep needs.
fn os_mask(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let mask = profile_points(a, "mask")?;
  let out = a.bool_or("out", false);
  let origins: Vec<usize> = (0..mask.len())
    .filter(|&i| mask[i][0] < 0.0 && mask[i][1] < 0.0)
    .collect();
  if origins.len() != 1 {
    return a.err(
      "cannot find the mask's origin: exactly one point must have both a \
       negative x and a negative y",
    );
  }
  let xfactor = if out { -1.0 } else { 1.0 };
  let start = origins[0];
  let points: Vec<[f64; 2]> = (0..mask.len())
    .map(|i| {
      let p = mask[(start + i) % mask.len()];
      [xfactor * p[0].max(0.0), -p[1].max(0.0)]
    })
    .collect();

  // Everything is measured from the first point off the origin, and repeated
  // points would make zero-length steps in the sweep.
  let base = points[1];
  let mut profile: Vec<[f64; 2]> = Vec::new();
  for p in &points[1..] {
    let q = [p[0] - base[0], p[1] - base[1]];
    if profile.last().is_none_or(|l: &[f64; 2]| {
      (l[0] - q[0]).abs() > EPS || (l[1] - q[1]).abs() > EPS
    }) {
      profile.push(q);
    }
  }

  let mut fields =
    vec![("extra", a.num("extra")), ("quality", a.num("quality"))];
  fields.retain(|(_, v)| v.is_some());
  let v = os_table(lua, "offset_sweep", "profile", &fields)?;
  if let LuaValue::Table(t) = &v {
    write_points(lua, t, &profile)?;
  }
  set_shared_rest(a, &v)?;
  Ok(v)
}

// -- The offset_stroke end treatments ---------------------------------------

fn os_pointed(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let Some(dist) = a.num("dist") else {
    return a.err("`dist` is required");
  };
  let t = lua.create_table()?;
  t.set("for", "offset_stroke")?;
  t.set("type", "shifted_point")?;
  t.set("loc", a.num_or("loc", 0.0))?;
  t.set("dist", dist)?;
  Ok(LuaValue::Table(t))
}

/// The angle an `os_round` or `os_flat` end is cut at, and whether it is
/// measured against the path or against the world.
fn stroke_angle(a: &Args) -> LuaResult<(f64, bool)> {
  match (a.num("angle"), a.num("abs_angle")) {
    (Some(_), Some(_)) => a.err("define only one of `angle` and `abs_angle`"),
    (Some(v), None) => Ok((v, false)),
    (None, Some(v)) => Ok((v, true)),
    (None, None) => Ok((0.0, false)),
  }
}

fn os_round(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  if a.has("r") {
    return a.err(
      "os_round takes no radius. Did you mean os_circle, which is the \
       offset_sweep roundover?",
    );
  }
  let (angle, absolute) = stroke_angle(a)?;
  // A roundover can be asymmetric, so `cut` reads as one value or two.
  let cut = match a.nums("cut") {
    Some(v) if v.len() >= 2 => [v[0], v[1]],
    _ => match a.num("cut") {
      Some(v) => [v, v],
      None => return a.err("`cut` is required"),
    },
  };
  let t = lua.create_table()?;
  t.set("for", "offset_stroke")?;
  t.set("type", "roundover")?;
  t.set("angle", angle)?;
  t.set("absolute", absolute)?;
  t.set("cut", Val::vec(cut).to_lua(lua)?)?;
  t.set("k", a.num_or("k", 0.75))?;
  Ok(LuaValue::Table(t))
}

fn os_flat(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let (angle, absolute) = stroke_angle(a)?;
  let t = lua.create_table()?;
  t.set("for", "offset_stroke")?;
  t.set("type", "flat")?;
  t.set("angle", angle)?;
  t.set("absolute", absolute)?;
  Ok(LuaValue::Table(t))
}

// ---------------------------------------------------------------------------
// angle_between_lines
// ---------------------------------------------------------------------------

/// The turn, in (-90, 90], that maps one line onto another.
///
/// Lines have no direction here, so the result folds into a quarter turn
/// either way rather than distinguishing a line from its reverse.
fn angle_between_lines(_lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let l1 = a.need_matrix("line1")?;
  let l2 = a.need_matrix("line2")?;
  if l1.len() < 2 || l2.len() < 2 {
    return a.err("each line must be given as two points");
  }
  let d1 = [l1[1][0] - l1[0][0], l1[1][1] - l1[0][1]];
  let d2 = [l2[1][0] - l2[0][0], l2[1][1] - l2[0][1]];
  let det = d1[0] * d2[1] - d1[1] * d2[0];
  let dot = d1[0] * d2[0] + d1[1] * d2[1];
  let mut angle = det.atan2(dot).to_degrees();
  if angle > 90.0 {
    angle -= 180.0;
  } else if angle <= -90.0 {
    angle += 180.0;
  }
  Ok(LuaValue::Number(angle))
}

// ---------------------------------------------------------------------------
// join_prism
// ---------------------------------------------------------------------------

use crate::bosl::vecmath::{self as vm, Mat4, V3};

/// The surface a prism is being joined to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Surface {
  /// The prism simply ends, with no fillet and a flat cap.
  None,
  /// The XY plane of the surface's own frame.
  Plane,
  /// A cylinder about the surface frame's X axis.
  Cylinder,
  /// A sphere about the surface frame's origin.
  Sphere,
}

impl Surface {
  fn parse(name: &str) -> Option<Surface> {
    match name {
      "none" => Some(Surface::None),
      "plane" => Some(Surface::Plane),
      "cyl" | "cylinder" => Some(Surface::Cylinder),
      "sphere" => Some(Surface::Sphere),
      _ => None,
    }
  }

  fn curved(self) -> bool {
    matches!(self, Surface::Cylinder | Surface::Sphere)
  }
}

/// The unit tangent at each point of a closed path.
fn path_tangents(path: &[V3]) -> Vec<V3> {
  let n = path.len();
  (0..n)
    .map(|i| {
      let prev = path[(i + n - 1) % n];
      let next = path[(i + 1) % n];
      vm::unit_or(vm::sub(next, prev), [0.0, 0.0, 1.0])
    })
    .collect()
}

/// The outward normal at each point of a closed 2D path, wound clockwise.
///
/// BOSL2 takes the tangent a quarter turn the other way, which points into a
/// clockwise outline; the caller negates it to get back out.
fn path_normals_2d(path: &[[f64; 2]]) -> Vec<[f64; 2]> {
  let n = path.len();
  (0..n)
    .map(|i| {
      let prev = path[(i + n - 1) % n];
      let next = path[(i + 1) % n];
      let t = [next[0] - prev[0], next[1] - prev[1]];
      let len = (t[0] * t[0] + t[1] * t[1]).sqrt();
      if len < EPS {
        [0.0, 0.0]
      } else {
        [t[1] / len, -t[0] / len]
      }
    })
    .collect()
}

/// The control points of a continuous-curvature corner in 3D.
fn smooth_bez_fill3(p: [V3; 3], k: f64) -> [V3; 5] {
  [
    p[0],
    vm::lerp3(p[1], p[0], k),
    p[1],
    vm::lerp3(p[1], p[2], k),
    p[2],
  ]
}

/// Sample a Bézier at `n + 1` points, both ends included.
fn bezier_curve3(ctrl: &[V3], n: usize) -> Vec<V3> {
  (0..=n)
    .map(|i| {
      let u = i as f64 / n as f64;
      let mut pts = ctrl.to_vec();
      while pts.len() > 1 {
        pts = pts.windows(2).map(|w| vm::lerp3(w[0], w[1], u)).collect();
      }
      pts[0]
    })
    .collect()
}

/// Where the segment through `a` and `b` crosses the plane `z = 0`.
fn plane_z_intersection(a: V3, b: V3) -> Option<V3> {
  let dz = b[2] - a[2];
  if dz.abs() < EPS {
    return None;
  }
  let t = -a[2] / dz;
  Some(vm::add(a, vm::mul(vm::sub(b, a), t)))
}

/// Where the line through `a` and `b` meets a cylinder of radius `r` about
/// the X axis, taking whichever crossing lies furthest along `pref`.
fn cyl_line_intersection(r: f64, a: V3, b: V3, pref: V3) -> Option<V3> {
  let d = vm::sub(b, a);
  // Only the components across the axis matter.
  let qa = d[1] * d[1] + d[2] * d[2];
  let qb = 2.0 * (a[1] * d[1] + a[2] * d[2]);
  let qc = a[1] * a[1] + a[2] * a[2] - r * r;
  best_root(qa, qb, qc, a, d, pref)
}

/// Where the line through `a` and `b` meets a sphere of radius `r` about the
/// origin, taking whichever crossing lies furthest along `pref`.
fn sphere_line_intersection(r: f64, a: V3, b: V3, pref: V3) -> Option<V3> {
  let d = vm::sub(b, a);
  let qa = vm::dot(d, d);
  let qb = 2.0 * vm::dot(a, d);
  let qc = vm::dot(a, a) - r * r;
  best_root(qa, qb, qc, a, d, pref)
}

fn best_root(qa: f64, qb: f64, qc: f64, a: V3, d: V3, pref: V3) -> Option<V3> {
  if qa.abs() < EPS {
    return None;
  }
  let disc = qb * qb - 4.0 * qa * qc;
  if disc < 0.0 {
    return None;
  }
  let root = disc.sqrt();
  let mut best: Option<(f64, V3)> = None;
  for t in [(-qb - root) / (2.0 * qa), (-qb + root) / (2.0 * qa)] {
    let p = vm::add(a, vm::mul(d, t));
    let score = vm::dot(p, pref);
    if best.is_none_or(|(s, _)| score > s) {
      best = Some((score, p));
    }
  }
  best.map(|(_, p)| p)
}

/// One end's fillet settings.
struct JoinEnd {
  surface: Surface,
  radius: f64,
  fillet: f64,
  k: f64,
  n: usize,
  overlap: f64,
  uniform: bool,
  transform: Mat4,
}

/// Build the ring-by-ring mesh that blends the prism into one surface.
///
/// `bot` is the ring of prism points nearest the surface and `top` the ring a
/// little way along it. The result runs from the prism outward onto the
/// surface, so the caller stacks the two ends' meshes nose to tail.
fn prism_fillet(
  end: &JoinEnd,
  bot: &[V3],
  top: &[V3],
  what: &str,
) -> Result<Vec<Vec<V3>>, String> {
  if end.surface == Surface::None {
    return Ok(vec![bot.to_vec()]);
  }
  let r = end.radius;
  let d = end.fillet;

  // Where each edge of the prism meets the surface.
  let isect: Vec<V3> = (0..top.len())
    .map(|i| match end.surface {
      Surface::Plane => plane_z_intersection(top[i], bot[i]),
      Surface::Cylinder => cyl_line_intersection(
        r.abs(),
        top[i],
        bot[i],
        vm::mul(vm::sub(top[i], bot[i]), r.signum()),
      ),
      Surface::Sphere => sphere_line_intersection(
        r.abs(),
        top[i],
        bot[i],
        vm::mul(vm::sub(top[i], bot[i]), r.signum()),
      ),
      Surface::None => None,
    })
    .collect::<Option<Vec<V3>>>()
    .ok_or_else(|| format!("the prism does not fully intersect the {what}"))?;

  // Which way the prism leaves the surface.
  let dir = match end.surface {
    Surface::Plane => (top[0][2] - bot[0][2]).signum(),
    _ => 1.0,
  };

  if d.abs() < EPS {
    let mut rings = vec![isect.clone()];
    if end.overlap != 0.0 {
      rings.push(overlap_ring(&isect, end, dir));
    }
    return Ok(rings);
  }

  let normals = match end.surface {
    Surface::Plane => {
      let flat: Vec<[f64; 2]> = isect.iter().map(|p| [p[0], p[1]]).collect();
      path_normals_2d(&flat)
        .iter()
        .map(|v| [-v[0], -v[1], 0.0])
        .collect::<Vec<V3>>()
    }
    _ => Vec::new(),
  };
  let tangents = if end.surface.curved() {
    path_tangents(&isect)
  } else {
    Vec::new()
  };

  let mut columns: Vec<Vec<V3>> = Vec::with_capacity(isect.len());
  for i in 0..isect.len() {
    if vm::norm(vm::sub(top[i], isect[i])) < d.abs() {
      return Err(format!(
        "the prism is too short for a {d} fillet to fit against the {what}"
      ));
    }
    let step = vm::add(
      isect[i],
      vm::mul(
        vm::unit_or(vm::sub(top[i], isect[i]), [0.0, 0.0, 1.0]),
        d.abs(),
      ),
    );

    let (corner, edgepoint) = match end.surface {
      Surface::Plane => {
        (isect[i], vm::add(isect[i], vm::mul(normals[i], d * dir)))
      }
      Surface::Cylinder => {
        // Walk `d` around the cylinder from the intersection, partly along
        // the axis and partly around it.
        let radial = [0.0, isect[i][1], isect[i][2]];
        let out = vm::mul(
          vm::unit_or(vm::cross(radial, tangents[i]), [1.0, 0.0, 0.0]),
          r.signum(),
        );
        let along = d * out[0];
        let around = d * (out[1] * out[1] + out[2] * out[2]).sqrt();
        let sign = (radial[1] * out[2] - radial[2] * out[1]).signum();
        let ang = sign * around / r.abs();
        let (s, c) = ang.sin_cos();
        let edge = [
          isect[i][0] + along,
          isect[i][1] * c - isect[i][2] * s,
          isect[i][1] * s + isect[i][2] * c,
        ];
        (
          surface_corner(edge, isect[i], top[i], [0.0, edge[1], edge[2]])?,
          edge,
        )
      }
      Surface::Sphere => {
        let out = vm::mul(
          vm::unit_or(vm::cross(isect[i], tangents[i]), [1.0, 0.0, 0.0]),
          r.signum(),
        );
        let ang = -d / r;
        let edge =
          Mat4::rot_by_axis(tangents[i], ang.to_degrees()).apply(isect[i]);
        let _ = out;
        (surface_corner(edge, isect[i], top[i], edge)?, edge)
      }
      Surface::None => unreachable!(),
    };

    let anchor = if end.uniform { isect[i] } else { corner };
    let d_step = if end.surface == Surface::Plane {
      step
    } else {
      vm::add(
        anchor,
        vm::mul(
          vm::unit_or(vm::sub(top[i], isect[i]), [0.0, 0.0, 1.0]),
          d.abs(),
        ),
      )
    };

    let bez = smooth_bez_fill3([d_step, corner, edgepoint], end.k);
    let mut column = bezier_curve3(&bez, end.n);
    if end.overlap != 0.0 {
      column.push(overlap_point(edgepoint, end, dir));
    }
    columns.push(column);
  }

  // The columns run along the prism; the mesh wants rings around it.
  let depth = columns[0].len();
  if columns.iter().any(|c| c.len() != depth) {
    return Err(format!("the {what} fillet came out uneven"));
  }
  Ok(
    (0..depth)
      .map(|row| columns.iter().map(|c| c[row]).collect())
      .collect(),
  )
}

/// Where the plane tangent to the surface at `edge` meets the prism edge.
fn surface_corner(
  edge: V3,
  isect: V3,
  top: V3,
  normal: V3,
) -> Result<V3, String> {
  let n = vm::unit_or(normal, [0.0, 0.0, 1.0]);
  let denom = vm::dot(n, vm::sub(top, isect));
  if denom.abs() < EPS {
    return Err("the fillet does not fit; reduce its size".to_string());
  }
  let t = vm::dot(n, vm::sub(edge, isect)) / denom;
  Ok(vm::add(isect, vm::mul(vm::sub(top, isect), t)))
}

/// A ring pushed just inside the surface, so the join overlaps rather than
/// meeting it exactly.
fn overlap_ring(ring: &[V3], end: &JoinEnd, dir: f64) -> Vec<V3> {
  ring.iter().map(|p| overlap_point(*p, end, dir)).collect()
}

fn overlap_point(p: V3, end: &JoinEnd, dir: f64) -> V3 {
  match end.surface {
    Surface::Plane | Surface::None => [p[0], p[1], p[2] - end.overlap * dir],
    Surface::Cylinder => {
      let rad = (p[1] * p[1] + p[2] * p[2]).sqrt();
      if rad < EPS {
        return p;
      }
      let s = (rad - end.radius.signum() * end.overlap) / rad;
      [p[0], p[1] * s, p[2] * s]
    }
    Surface::Sphere => {
      let n = vm::norm(p);
      if n < EPS {
        return p;
      }
      let s = (n - end.radius.signum() * end.overlap) / n;
      vm::mul(p, s)
    }
  }
}

/// Read a closed outline, given either as points or as a 2D shape.
///
/// Taking a sketch is what lets `bosl.circle{r=15}` be swept or joined
/// directly, rather than having its points written out by hand.
pub fn read_outline(a: &Args, name: &str) -> Option<Vec<[f64; 2]>> {
  if let Some(LuaValue::UserData(ud)) = a.raw(name)
    && let Ok(sketch) = ud.borrow::<crate::geometry::CsgSketch>()
  {
    return crate::bosl::sweeps::outline_of(sketch.scad.as_ref());
  }
  a.points2(name).filter(|p| p.len() >= 3)
}

/// Read a 4×4 transformation matrix parameter, defaulting to the identity.
fn matrix_arg(a: &Args, name: &str) -> LuaResult<Mat4> {
  let Some(v) = a.val(name) else {
    return Ok(Mat4::identity());
  };
  let Some(rows) = v.as_matrix() else {
    return a.err(format!("{name} must be a 4x4 matrix"));
  };
  if rows.len() != 4 || rows.iter().any(|r| r.len() != 4) {
    return a.err(format!("{name} must be a 4x4 matrix"));
  }
  let mut m = [0.0; 16];
  for (i, row) in rows.iter().enumerate() {
    m[i * 4..i * 4 + 4].copy_from_slice(row);
  }
  Ok(Mat4(m))
}

/// The inverse of a rigid transform — a rotation and a translation.
fn rigid_inverse(m: &Mat4) -> Mat4 {
  let r = m.0;
  // Transpose the rotation block and move the translation back through it.
  let t = [r[3], r[7], r[11]];
  let inv_t = [
    -(r[0] * t[0] + r[4] * t[1] + r[8] * t[2]),
    -(r[1] * t[0] + r[5] * t[1] + r[9] * t[2]),
    -(r[2] * t[0] + r[6] * t[1] + r[10] * t[2]),
  ];
  Mat4([
    r[0], r[4], r[8], inv_t[0], //
    r[1], r[5], r[9], inv_t[1], //
    r[2], r[6], r[10], inv_t[2], //
    0.0, 0.0, 0.0, 1.0,
  ])
}

/// Read one end's settings, falling back through the shared parameter names.
fn join_end(
  a: &Args,
  which: &str,
  surface: Surface,
  radius: f64,
  transform: Mat4,
  fillet: f64,
) -> LuaResult<JoinEnd> {
  let pick =
    |specific: &str, shared: &str| a.num(specific).or_else(|| a.num(shared));
  if surface.curved() && fillet < 0.0 {
    return a.err(format!(
      "a fillet against a {which} cylinder or sphere cannot be negative"
    ));
  }
  Ok(JoinEnd {
    surface,
    radius,
    fillet,
    k: pick(&format!("{which}_k"), "k").unwrap_or(0.7),
    n: pick(&format!("{which}_n"), "n").unwrap_or(15.0).max(1.0) as usize,
    overlap: pick(&format!("{which}_overlap"), "overlap")
      .unwrap_or(if fillet > 0.0 { 1.0 } else { 0.0 }),
    uniform: a
      .bool(&format!("{which}_uniform"))
      .or_else(|| a.bool("uniform"))
      .unwrap_or(true),
    transform,
  })
}

/// Join a prism onto another surface, blending the two with a fillet.
///
/// The prism is a 2D outline swept along its axis. Where it lands on the base
/// surface the two are blended by a fillet of the given radius, which is what
/// keeps the joint from being a sharp reentrant corner for cracks to start
/// from. A negative fillet against a plane rounds the joint the other way,
/// cutting into the prism instead of adding material around it.
fn join_prism(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let Some(polygon) = read_outline(a, "polygon") else {
    return a.err("polygon must be a 2D outline or a sketch");
  };
  if polygon.len() < 3 {
    return a.err("polygon must have at least three points");
  }
  // BOSL2 works from a clockwise outline, so its normals point outward.
  let area: f64 = crate::bosl::offset2d::signed_area2(&polygon);
  let polygon: Vec<[f64; 2]> = if area > 0.0 {
    polygon.iter().rev().copied().collect()
  } else {
    polygon
  };

  let base_name = a.string("base").unwrap_or_else(|| "plane".to_string());
  let Some(base) = Surface::parse(&base_name) else {
    return a
      .err("base must be \"plane\", \"cyl\", \"cylinder\" or \"sphere\"");
  };
  if base == Surface::None {
    return a.err("base must be a surface, not \"none\"");
  }
  let aux_name = a.string("aux").unwrap_or_else(|| "none".to_string());
  let Some(mut aux) = Surface::parse(&aux_name) else {
    return a.err(
      "aux must be \"none\", \"plane\", \"cyl\", \"cylinder\" or \"sphere\"",
    );
  };

  let base_r = a.radius("base_r", "base_d", None).unwrap_or(0.0);
  let aux_r = a.radius("aux_r", "aux_d", None).unwrap_or(0.0);
  if base.curved() && base_r.abs() < EPS {
    return a.err("a curved base needs a non-zero base_r");
  }
  if aux.curved() && aux_r.abs() < EPS {
    return a.err("a curved aux needs a non-zero aux_r");
  }

  let base_t = matrix_arg(a, "base_T")?;
  let mut aux_t = matrix_arg(a, "aux_T")?;
  let scale = a.num_or("scale", 1.0);
  if scale < 0.0 {
    return a.err("scale must be non-negative");
  }
  let length = a
    .num("length")
    .or_else(|| a.num("l"))
    .or_else(|| a.num("h"))
    .or_else(|| a.num("height"));

  // Where the prism starts on the base, and which way it runs.
  let dir = if aux == Surface::None {
    vm::unit_or(aux_t.apply([0.0, 0.0, 1.0]), [0.0, 0.0, 1.0])
  } else {
    let c = aux_t.apply([0.0, 0.0, 0.0]);
    if vm::norm(c) < EPS {
      vm::unit_or(aux_t.apply([0.0, 0.0, 1.0]), [0.0, 0.0, 1.0])
    } else {
      vm::unit_or(c, [0.0, 0.0, 1.0])
    }
  };

  let base_inv = rigid_inverse(&base_t);
  let axis_a = [0.0, 0.0, 0.0];
  let axis_b = dir;
  let start = match base {
    Surface::Plane | Surface::None => [0.0, 0.0, 0.0],
    Surface::Cylinder => cyl_line_intersection(
      base_r.abs(),
      axis_a,
      axis_b,
      vm::mul(dir, base_r.signum()),
    )
    .ok_or(())
    .or_else(|_| a.err("the prism's axis does not meet the base cylinder"))?,
    Surface::Sphere => sphere_line_intersection(
      base_r.abs(),
      axis_a,
      axis_b,
      vm::mul(dir, base_r.signum()),
    )
    .ok_or(())
    .or_else(|_| a.err("the prism's axis does not meet the base sphere"))?,
  };

  let end = if aux == Surface::None {
    let Some(length) = length else {
      return a.err("a prism with no aux surface needs a positive length");
    };
    if length <= 0.0 {
      return a.err("length must be positive");
    }
    vm::add(start, vm::mul(dir, length))
  } else {
    if length.is_some() {
      return a.err("length applies only when aux is \"none\"");
    }
    let centre = aux_t.apply([0.0, 0.0, 0.0]);
    let ndir = vm::unit_or(vm::sub(centre, start), [0.0, 0.0, 1.0]);
    let aux_inv = rigid_inverse(&aux_t);
    let a0 = aux_inv.apply(start);
    let a1 = aux_inv.apply(vm::add(start, ndir));
    let hit = match aux {
      Surface::Plane | Surface::None => plane_z_intersection(a0, a1),
      Surface::Cylinder => cyl_line_intersection(
        aux_r.abs(),
        a0,
        a1,
        vm::mul(vm::sub(a0, a1), aux_r.signum()),
      ),
      Surface::Sphere => sphere_line_intersection(
        aux_r.abs(),
        a0,
        a1,
        vm::mul(vm::sub(a0, a1), aux_r.signum()),
      ),
    };
    let Some(hit) = hit else {
      return a.err("the prism's axis does not meet the aux surface");
    };
    aux_t.apply(hit)
  };

  let axis = vm::sub(end, start);
  let run = vm::norm(axis);
  if run < EPS {
    return a.err("the prism has no length");
  }

  let base_fillet = a.num("base_fillet").or_else(|| a.num("fillet"));
  let Some(base_fillet) = base_fillet else {
    return a.err("a numeric fillet or base_fillet is required");
  };
  let base_end = join_end(a, "base", base, base_r, base_t, base_fillet)?;

  // With no aux surface the far end is a flat cap, unless it is being
  // rounded — and then it needs a plane to round against, standing across
  // the axis at the end of the prism. `end_round` measures that rounding the
  // other way round, so it arrives here negated.
  let aux_fillet = if aux == Surface::None {
    let rounding = a
      .num("aux_fillet")
      .or_else(|| a.num("end_round").map(|v| -v))
      .unwrap_or(0.0);
    if rounding != 0.0 {
      aux = Surface::Plane;
      aux_t =
        Mat4::translate(end).mul(&Mat4::rot_from_to([0.0, 0.0, 1.0], axis));
    }
    rounding
  } else {
    match a.num("aux_fillet").or_else(|| a.num("fillet")) {
      Some(v) => v,
      None => return a.err("a numeric fillet or aux_fillet is required"),
    }
  };
  let aux_end = join_end(a, "aux", aux, aux_r, aux_t, aux_fillet)?;

  // The prism itself: the outline at the base, and a scaled copy at the far
  // end, both turned to stand along the axis.
  let pangle = Mat4::rot_from_to([0.0, 0.0, 1.0], axis);
  let place = Mat4::translate(start).mul(&pangle);
  let truebot: Vec<V3> = polygon
    .iter()
    .map(|p| place.apply([p[0], p[1], 0.0]))
    .collect();
  let truetop: Vec<V3> = polygon
    .iter()
    .map(|p| place.apply([p[0] * scale, p[1] * scale, run]))
    .collect();

  let base_bot: Vec<V3> = truebot.iter().map(|p| base_inv.apply(*p)).collect();
  let base_top: Vec<V3> = truetop.iter().map(|p| base_inv.apply(*p)).collect();
  let botmesh = prism_fillet(&base_end, &base_bot, &base_top, "base")
    .or_else(|e| a.err(e))?;
  let botmesh: Vec<Vec<V3>> = botmesh
    .iter()
    .map(|ring| ring.iter().map(|p| base_t.apply(*p)).collect())
    .collect();

  // The far end is the same construction seen from the other direction, so
  // the outline is reversed going in and the rings come back reversed too.
  let aux_inv = rigid_inverse(&aux_end.transform);
  let rev = |ring: &[V3]| -> Vec<V3> {
    ring.iter().rev().map(|p| aux_inv.apply(*p)).collect()
  };
  let aux_top = rev(&truetop);
  let aux_bot = rev(&truebot);
  let topmesh =
    prism_fillet(&aux_end, &aux_top, &aux_bot, "aux").or_else(|e| a.err(e))?;
  let topmesh: Vec<Vec<V3>> = topmesh
    .iter()
    .rev()
    .map(|ring| {
      ring
        .iter()
        .rev()
        .map(|p| aux_end.transform.apply(*p))
        .collect()
    })
    .collect();

  let mut rows = topmesh;
  rows.extend(botmesh);
  if rows.len() < 2 {
    return a.err("the join produced no surface");
  }
  // The rings run from the far end of the prism back down to the base, which
  // is the opposite of the direction `vertex_array` takes as outward, so the
  // surface comes out facing the right way with no further reversal.
  let vnf = crate::bosl::vnf::Vnf::vertex_array(
    &rows,
    crate::bosl::vnf::Caps::BOTH,
    true,
    false,
  );

  let scad = crate::bosl::bosl_node_with_children(
    "rounding.scad",
    "join_prism",
    a.scad_args().to_string(),
    vec![],
    Some(vnf.to_node()),
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
// bent_cutout_mask
// ---------------------------------------------------------------------------

/// A mask that cuts a flat outline through the wall of a round tube.
///
/// The outline is given as if the tube had been unrolled: x runs around the
/// circumference and y along the axis. Wrapping it back onto the cylinder is
/// what makes the cut come out the right shape once the tube is round again.
/// The mask is a half-round bar following the outline, so the cut it leaves
/// has rounded edges on both faces of the wall.
fn bent_cutout_mask(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  use crate::bosl::offset2d::{Corners, JoinStyle};

  let Some(r) = a.radius("r", "d", None).or_else(|| a.num("radius")) else {
    return a.err("r is required: the radius of the tube to bend around");
  };
  let thickness = a.num_or("thickness", 0.0);
  if r <= 0.0 {
    return a.err("the radius of the tube to bend around must be positive");
  }
  if thickness <= 0.0 {
    return a.err("thickness must be positive");
  }
  if r - thickness <= 0.0 {
    return a.err("thickness is too large for the radius");
  }
  let Some(path) = read_outline(a, "path") else {
    return a.err("path must be a 2D outline or a sketch");
  };
  if path.len() < 3 {
    return a.err("path must have at least three points");
  }
  // BOSL2 works from a clockwise outline.
  let path: Vec<[f64; 2]> = if crate::bosl::offset2d::signed_area2(&path) > 0.0
  {
    path.iter().rev().copied().collect()
  } else {
    path
  };

  let span_x =
    |f: fn(f64, f64) -> f64, init: f64| path.iter().map(|p| p[0]).fold(init, f);
  let min_angle = (span_x(f64::min, f64::INFINITY) - thickness / 2.0) * 360.0
    / (2.0 * PI_F * r);
  let max_angle = (span_x(f64::max, f64::NEG_INFINITY) + thickness / 2.0)
    * 360.0
    / (2.0 * PI_F * r);
  if max_angle - min_angle >= 180.0 {
    return a.err("the cutout spans too far around the tube; it must stay under half a turn");
  }
  let mid_angle = (max_angle + min_angle) / 2.0;
  let min_dist =
    (r + thickness / 2.0) / ((max_angle - min_angle) / 2.0).to_radians().cos();
  let z_mean = path.iter().map(|p| p[1]).sum::<f64>() / path.len() as f64;

  // The bar's half-round cross-section, swept along the outline. The outline
  // is offset with mitred corners rather than rounded ones, so a square
  // cutout keeps its four corners square.
  let segs = a.segments(thickness / 2.0);
  let arc_points = ((segs as f64 / 2.0).ceil() as u32).max(3);
  let corners = Corners::plan(&path, JoinStyle::Delta, thickness, segs);
  let mut rows: Vec<Vec<V3>> = Vec::new();
  rows.push(vec![[0.0, 0.0, z_mean]; corners.point_count()]);
  for i in 0..arc_points {
    // Half a turn, from the inside of the wall round to the outside.
    let ang = PI_F * (i as f64 / (arc_points - 1) as f64) - PI_F;
    let (dy, dx) = ang.sin_cos();
    let radius = r + thickness / 2.0 * dx;
    // Widest where the bar meets each face of the wall and pinched to the
    // outline itself at mid-wall, which is what rounds both edges of the cut.
    let widen = thickness / 2.0 * (1.0 + dy);
    let ring = corners.offset(widen, JoinStyle::Delta);
    rows.push(
      ring
        .iter()
        .map(|p| {
          // x measured arc length around the unrolled tube, so it converts
          // to an angle at the radius the outline was drawn for, not at this
          // layer's radius. That keeps the cut lined up through the wall
          // instead of shearing as it crosses.
          let (s, c) = (p[0] / r).sin_cos();
          [radius * c, radius * s, p[1]]
        })
        .collect(),
    );
  }
  rows.push(vec![
    [
      1.5 * min_dist * mid_angle.to_radians().cos(),
      1.5 * min_dist * mid_angle.to_radians().sin(),
      z_mean,
    ];
    corners.point_count()
  ]);

  let vnf = crate::bosl::vnf::Vnf::vertex_array(
    &rows,
    crate::bosl::vnf::Caps::NONE,
    true,
    false,
  );
  let scad = crate::bosl::bosl_node_with_children(
    "rounding.scad",
    "bent_cutout_mask",
    a.scad_args().to_string(),
    vec![],
    Some(vnf.to_node()),
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

const PI_F: f64 = std::f64::consts::PI;

// ---------------------------------------------------------------------------
// Registration
// ---------------------------------------------------------------------------

pub fn register(lua: &Lua, bosl: &mlua::Table) -> LuaResult<()> {
  use crate::bosl::value::register_pure;

  // `register_pure` wants a `'static` slice, so each parameter list is
  // spelled out rather than built from a shared tail.
  register_pure(
    lua,
    bosl,
    "os_circle",
    &[
      "r",
      "cut",
      "extra",
      "check_valid",
      "quality",
      "steps",
      "offset",
    ],
    os_circle,
  )?;
  register_pure(
    lua,
    bosl,
    "os_teardrop",
    &[
      "r",
      "cut",
      "extra",
      "check_valid",
      "quality",
      "steps",
      "offset",
    ],
    os_teardrop,
  )?;
  register_pure(
    lua,
    bosl,
    "os_chamfer",
    &[
      "height",
      "width",
      "cut",
      "angle",
      "extra",
      "check_valid",
      "quality",
      "steps",
      "offset",
    ],
    os_chamfer,
  )?;
  register_pure(
    lua,
    bosl,
    "os_smooth",
    &[
      "cut",
      "joint",
      "k",
      "extra",
      "check_valid",
      "quality",
      "steps",
      "offset",
    ],
    os_smooth,
  )?;
  register_pure(
    lua,
    bosl,
    "os_profile",
    &["points", "extra", "check_valid", "quality", "offset"],
    os_profile,
  )?;
  register_pure(
    lua,
    bosl,
    "os_mask",
    &["mask", "out", "extra", "check_valid", "quality", "offset"],
    os_mask,
  )?;
  register_pure(lua, bosl, "os_pointed", &["dist", "loc"], os_pointed)?;
  register_pure(
    lua,
    bosl,
    "os_round",
    &["cut", "angle", "abs_angle", "k", "r"],
    os_round,
  )?;
  register_pure(lua, bosl, "os_flat", &["angle", "abs_angle"], os_flat)?;
  register_pure(
    lua,
    bosl,
    "angle_between_lines",
    &["line1", "line2"],
    angle_between_lines,
  )?;
  register_pure(
    lua,
    bosl,
    "join_prism",
    &[
      "polygon",
      "base",
      "base_r",
      "base_d",
      "base_T",
      "scale",
      "prism_end_T",
      "short",
      "length",
      "l",
      "height",
      "h",
      "aux",
      "aux_T",
      "aux_r",
      "aux_d",
      "overlap",
      "base_overlap",
      "aux_overlap",
      "n",
      "base_n",
      "end_n",
      "aux_n",
      "fillet",
      "base_fillet",
      "aux_fillet",
      "end_round",
      "k",
      "base_k",
      "aux_k",
      "end_k",
      "start",
      "end",
      "uniform",
      "base_uniform",
      "aux_uniform",
      "debug",
    ],
    join_prism,
  )?;
  register_pure(
    lua,
    bosl,
    "bent_cutout_mask",
    &["r", "thickness", "path", "radius", "d", "convexity"],
    bent_cutout_mask,
  )?;
  Ok(())
}

#[cfg(test)]
mod tests {
  use super::*;

  fn spec(edge_type: EdgeType) -> EdgeSpec {
    EdgeSpec {
      edge_type,
      steps: 4,
      ..EdgeSpec::default()
    }
  }

  #[test]
  fn a_roundover_draws_the_outline_in_as_it_rises() {
    let s = EdgeSpec {
      r: Some(2.0),
      ..spec(EdgeType::Circle)
    };
    let o = rounding_offsets(&s, 1.0).unwrap();
    assert_eq!(o.len(), 4);
    // A quarter turn, so the last step is a full radius in and up.
    assert!((o[3][0] - -2.0).abs() < 1e-6, "{o:?}");
    assert!((o[3][1] - 2.0).abs() < 1e-6, "{o:?}");
    // Every step draws inward.
    assert!(o.iter().all(|p| p[0] <= 1e-9), "{o:?}");
  }

  #[test]
  fn a_negative_radius_flares_the_outline_outward() {
    let s = EdgeSpec {
      r: Some(-2.0),
      ..spec(EdgeType::Circle)
    };
    let o = rounding_offsets(&s, 1.0).unwrap();
    // The rise is still upward, but the outline grows rather than shrinks.
    assert!(o.iter().all(|p| p[0] >= -1e-9), "{o:?}");
    assert!((o[3][0] - 2.0).abs() < 1e-6, "{o:?}");
    assert!((o[3][1] - 2.0).abs() < 1e-6, "{o:?}");
  }

  #[test]
  fn the_bottom_end_mirrors_the_top() {
    let s = EdgeSpec {
      r: Some(2.0),
      ..spec(EdgeType::Circle)
    };
    let top = rounding_offsets(&s, 1.0).unwrap();
    let bot = rounding_offsets(&s, -1.0).unwrap();
    for (t, b) in top.iter().zip(bot.iter()) {
      assert!((t[0] - b[0]).abs() < 1e-9, "{top:?} {bot:?}");
      assert!((t[1] + b[1]).abs() < 1e-9, "{top:?} {bot:?}");
    }
  }

  #[test]
  fn a_cut_measures_the_corner_taken_out_rather_than_the_radius() {
    let by_cut = EdgeSpec {
      cut: Some(1.0),
      ..spec(EdgeType::Circle)
    };
    let r = 1.0 / (std::f64::consts::SQRT_2 - 1.0);
    let by_radius = EdgeSpec {
      r: Some(r),
      ..spec(EdgeType::Circle)
    };
    assert_eq!(
      rounding_offsets(&by_cut, 1.0).unwrap(),
      rounding_offsets(&by_radius, 1.0).unwrap()
    );
  }

  #[test]
  fn a_chamfer_is_a_single_straight_step() {
    let s = EdgeSpec {
      chamfer_width: Some(3.0),
      angle: 45.0,
      ..spec(EdgeType::Chamfer)
    };
    let o = rounding_offsets(&s, 1.0).unwrap();
    assert_eq!(o.len(), 1);
    assert!((o[0][0] - -3.0).abs() < 1e-6, "{o:?}");
    assert!((o[0][1] - 3.0).abs() < 1e-6, "{o:?}");
  }

  #[test]
  fn a_chamfer_needs_two_of_its_three_measurements() {
    let s = spec(EdgeType::Chamfer);
    assert!(rounding_offsets(&s, 1.0).is_err());
  }

  #[test]
  fn extra_carries_the_profile_past_its_own_end() {
    let s = EdgeSpec {
      r: Some(2.0),
      extra: 0.5,
      ..spec(EdgeType::Circle)
    };
    let o = rounding_offsets(&s, 1.0).unwrap();
    assert_eq!(o.len(), 5);
    // Straight on past the last step, without drawing further in.
    assert!((o[4][0] - o[3][0]).abs() < 1e-9, "{o:?}");
    assert!((o[4][1] - (o[3][1] + 0.5)).abs() < 1e-9, "{o:?}");
  }

  #[test]
  fn a_teardrop_stops_at_45_degrees_so_it_prints_without_support() {
    let s = EdgeSpec {
      r: Some(2.0),
      ..spec(EdgeType::Teardrop)
    };
    let o = rounding_offsets(&s, 1.0).unwrap();
    // The arc, then the straight run out to full height.
    assert_eq!(o.len(), 5);
    assert!((o[4][1] - 2.0).abs() < 1e-6, "{o:?}");
    let arc_top = o[3];
    assert!(
      (arc_top[1] - 2.0 * (45f64.to_radians()).sin()).abs() < 1e-3,
      "{o:?}"
    );
  }

  #[test]
  fn a_profile_must_start_on_the_outline() {
    let s = EdgeSpec {
      points: vec![[1.0, 1.0], [2.0, 2.0]],
      ..spec(EdgeType::Profile)
    };
    assert!(rounding_offsets(&s, 1.0).is_err());
  }

  #[test]
  fn a_smooth_roundover_ends_a_joint_away_in_both_directions() {
    let s = EdgeSpec {
      joint: Some(2.0),
      ..spec(EdgeType::Smooth)
    };
    let o = rounding_offsets(&s, 1.0).unwrap();
    let last = *o.last().unwrap();
    assert!((last[0] - -2.0).abs() < 1e-6, "{o:?}");
    assert!((last[1] - 2.0).abs() < 1e-6, "{o:?}");
  }
}
