//! BOSL2's `gears.scad`: involute spur, helical, bevel, worm and rack gears.
//!
//! A gear is described by its tooth count and its size, and the size can be
//! given four interchangeable ways: the circular pitch (arc length between
//! teeth at the pitch circle), the module (millimetres of pitch diameter per
//! tooth), the diametral pitch (teeth per inch of diameter), or the older
//! `pitch` spelling of the circular pitch. Everything below works from the
//! circular pitch, converting on the way in.

use std::f64::consts::PI;

use mlua::{Lua, Result as LuaResult, Value as LuaValue};

use crate::bosl::attach::{Attachable, Geom, reorient};
use crate::bosl::value::Args;
use crate::bosl::vecmath::Mat4;
use crate::bosl::vnf::{Caps, Vnf};
use crate::geometry::{CsgGeometry, CsgSketch};
use crate::scad_export::ScadNode;

/// Millimetres per inch, for the diametral pitch conversion.
const INCH: f64 = 25.4;

fn as_geometry(
  lua: &Lua,
  function: &'static str,
  native: ScadNode,
) -> LuaResult<LuaValue> {
  let scad = crate::bosl::bosl_node_with_children(
    "gears.scad",
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

fn as_sketch(
  lua: &Lua,
  function: &'static str,
  native: ScadNode,
) -> LuaResult<LuaValue> {
  let scad = crate::bosl::bosl_node_with_children(
    "gears.scad",
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
    material: None,
    scad: Some(scad),
  })?))
}

// ---------------------------------------------------------------------------
// The four ways of naming a gear's size
// ---------------------------------------------------------------------------

/// The circular pitch, whichever way the size was given.
fn circ_pitch_of(a: &Args) -> LuaResult<f64> {
  if let Some(p) = a.num("pitch") {
    return ok_positive(a, p, "pitch");
  }
  if let Some(p) = a.num("circ_pitch") {
    return ok_positive(a, p, "circ_pitch");
  }
  if let Some(dp) = a.num("diam_pitch") {
    let dp = ok_positive(a, dp, "diam_pitch")?;
    return Ok(PI / dp * INCH);
  }
  if let Some(m) = a.num("mod") {
    return Ok(ok_positive(a, m, "mod")? * PI);
  }
  a.err("give one of circ_pitch, mod, pitch or diam_pitch")
}

fn ok_positive(a: &Args, v: f64, name: &str) -> LuaResult<f64> {
  if v > 0.0 {
    Ok(v)
  } else {
    a.err(format!("{name} must be positive"))
  }
}

/// The module: millimetres of pitch diameter per tooth.
fn module_of(circ_pitch: f64) -> f64 {
  circ_pitch / PI
}

/// The radius of the circle the teeth roll on.
fn pitch_radius_of(circ_pitch: f64, teeth: f64, helical: f64) -> f64 {
  circ_pitch * teeth / PI / 2.0 / helical.to_radians().cos()
}

/// How far the tooth reaches beyond the pitch circle.
fn addendum(circ_pitch: f64, profile_shift: f64, shorten: f64) -> f64 {
  module_of(circ_pitch) * (1.0 + profile_shift - shorten)
}

/// How far the root sits inside the pitch circle.
fn dedendum(
  circ_pitch: f64,
  clearance: Option<f64>,
  profile_shift: f64,
) -> f64 {
  let m = module_of(circ_pitch);
  let clearance = clearance.unwrap_or(0.25 * m);
  m * (1.0 - profile_shift) + clearance
}

/// The shift applied to a small gear so its teeth do not undercut.
///
/// Below about 19 teeth an unshifted involute tooth is cut away at the root
/// by the tool that forms it; shifting the profile outward avoids that.
fn auto_profile_shift(
  teeth: f64,
  pressure_angle: f64,
  given: Option<f64>,
) -> f64 {
  if let Some(v) = given {
    return v;
  }
  let min_teeth = 2.0 / pressure_angle.to_radians().sin().powi(2);
  if teeth >= min_teeth {
    0.0
  } else {
    1.0 - teeth / min_teeth
  }
}

/// The involute of a circle, as the point at angle `a` along it.
fn involute(base_r: f64, a: f64) -> [f64; 2] {
  let rad = a.to_radians();
  let (s, c) = rad.sin_cos();
  [base_r * (c + rad * s), base_r * (s - rad * c)]
}

// ---------------------------------------------------------------------------
// The tooth profile
// ---------------------------------------------------------------------------

struct GearSpec {
  circ_pitch: f64,
  teeth: f64,
  pressure_angle: f64,
  clearance: Option<f64>,
  backlash: f64,
  helical: f64,
  profile_shift: f64,
  shorten: f64,
  internal: bool,
}

impl GearSpec {
  fn read(a: &Args) -> LuaResult<GearSpec> {
    let circ_pitch = circ_pitch_of(a)?;
    let teeth = a.need_num("teeth")?;
    if teeth <= 3.0 {
      return a.err("a gear needs more than three teeth");
    }
    let pressure_angle = a.num_or("pressure_angle", 20.0);
    Ok(GearSpec {
      circ_pitch,
      teeth,
      pressure_angle,
      clearance: a.num("clearance"),
      backlash: a.num_or("backlash", 0.0),
      helical: a.num_or("helical", 0.0),
      profile_shift: auto_profile_shift(
        teeth,
        pressure_angle,
        a.num("profile_shift"),
      ),
      shorten: a.num_or("shorten", 0.0),
      internal: a.bool_or("internal", false),
    })
  }

  fn pitch_radius(&self) -> f64 {
    pitch_radius_of(self.circ_pitch, self.teeth, self.helical)
  }

  fn outer_radius(&self) -> f64 {
    self.pitch_radius()
      + if self.internal {
        dedendum(self.circ_pitch, self.clearance, -self.profile_shift)
      } else {
        addendum(self.circ_pitch, self.profile_shift, self.shorten)
      }
  }

  fn root_radius(&self) -> f64 {
    self.pitch_radius()
      - if self.internal {
        addendum(self.circ_pitch, -self.profile_shift, self.shorten)
      } else {
        dedendum(self.circ_pitch, self.clearance, self.profile_shift)
      }
  }

  /// One tooth of the gear, as a path from one root to the next.
  ///
  /// The flanks are involutes of the base circle, which is what makes the
  /// contact between two gears roll rather than slide.
  fn tooth(&self, steps: usize) -> Vec<[f64; 2]> {
    let pr = self.pitch_radius();
    let or = self.outer_radius();
    let rr = self.root_radius();
    let base_r = pr * self.pressure_angle.to_radians().cos();
    let m = module_of(self.circ_pitch);

    // Where the involute crosses a given radius.
    let angle_at = |r: f64| -> f64 {
      if r <= base_r {
        0.0
      } else {
        ((r / base_r).powi(2) - 1.0).sqrt().to_degrees()
      }
    };
    // The involute's own polar angle, which the tooth is centred against.
    let polar = |r: f64| -> f64 {
      let a = angle_at(r);
      a - a.to_radians().tan().to_degrees().atan().to_degrees() * 0.0
        - involute_angle(base_r, r)
    };
    let _ = polar;

    // Half the tooth's angular width at the pitch circle, allowing for the
    // profile shift and the backlash.
    let half_thick = self.circ_pitch / 4.0
      + self.profile_shift * m * self.pressure_angle.to_radians().tan()
      - self.backlash / 2.0;
    let half_angle = half_thick / pr * 180.0 / PI;
    let centre = half_angle + involute_angle(base_r, pr).to_degrees();

    // One flank, from the root out to the tip.
    let start_r = rr.max(base_r);
    let flank: Vec<[f64; 2]> = (0..=steps)
      .map(|i| {
        let r = start_r + (or - start_r) * i as f64 / steps as f64;
        let a = angle_at(r);
        let p = involute(base_r, a);
        // Turn the involute so the tooth straddles the X axis.
        let turn = -centre;
        let (s, c) = turn.to_radians().sin_cos();
        [p[0] * c - p[1] * s, p[0] * s + p[1] * c]
      })
      .collect();

    // The root below the base circle runs straight in, and the other flank
    // is this one mirrored.
    let root_angle = 180.0 / self.teeth;
    let (rs, rc) = root_angle.to_radians().sin_cos();
    let mut path: Vec<[f64; 2]> = vec![[rr * rc, -rr * rs]];
    if rr < start_r {
      let first = flank[0];
      path.push([first[0], first[1]]);
    }
    path.extend(flank.iter().map(|p| [p[0], -p[1]]).rev());
    path.extend(flank.iter().copied());
    if rr < start_r {
      let first = flank[0];
      path.push([first[0], -first[1]]);
    }
    path.push([rr * rc, rr * rs]);
    // The tooth is built pointing along +X and mirrored about it, so the
    // two halves already meet; drop the duplicated tip.
    crate::bosl::shapes2d::dedup_closed(path)
  }

  /// The whole outline, one tooth repeated round the gear.
  fn outline(&self, steps: usize, hide: usize, spin: f64) -> Vec<[f64; 2]> {
    let tooth = self.tooth(steps);
    let n = self.teeth as usize;
    let mut out = Vec::with_capacity(tooth.len() * n);
    for i in 0..n.saturating_sub(hide) {
      let ang = -(i as f64) * 360.0 / self.teeth + spin;
      let (s, c) = ang.to_radians().sin_cos();
      out.extend(
        tooth
          .iter()
          .map(|p| [p[0] * c - p[1] * s, p[0] * s + p[1] * c]),
      );
    }
    if hide > 0 {
      out.push([0.0, 0.0]);
    }
    crate::bosl::shapes2d::dedup_closed(out)
  }
}

/// The involute's polar angle at a given radius.
fn involute_angle(base_r: f64, r: f64) -> f64 {
  if r <= base_r {
    return 0.0;
  }
  let a = ((r / base_r).powi(2) - 1.0).sqrt();
  a - a.atan()
}

// ---------------------------------------------------------------------------
// The size functions
// ---------------------------------------------------------------------------

fn circular_pitch(_lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  Ok(LuaValue::Number(circ_pitch_of(a)?))
}

fn diametral_pitch(_lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  // The exact inverse of the conversion the other way, so the four ways of
  // naming a size round-trip.
  Ok(LuaValue::Number(PI * INCH / circ_pitch_of(a)?))
}

fn module_value(_lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  Ok(LuaValue::Number(module_of(circ_pitch_of(a)?)))
}

fn pitch_radius(_lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let circ_pitch = circ_pitch_of(a)?;
  let teeth = a.need_num("teeth")?;
  Ok(LuaValue::Number(pitch_radius_of(
    circ_pitch,
    teeth,
    a.num_or("helical", 0.0),
  )))
}

fn outer_radius(_lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  Ok(LuaValue::Number(GearSpec::read(a)?.outer_radius()))
}

fn root_radius(_lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  Ok(LuaValue::Number(GearSpec::read(a)?.root_radius()))
}

fn auto_profile_shift_fn(_lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let teeth = a.need_num("teeth")?;
  Ok(LuaValue::Number(auto_profile_shift(
    teeth,
    a.num_or("pressure_angle", 20.0),
    a.num("profile_shift"),
  )))
}

fn get_profile_shift(_lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let desired = a.need_num("desired")?;
  let t1 = a.need_num("teeth1")?;
  let t2 = a.need_num("teeth2")?;
  let m = module_of(circ_pitch_of(a)?);
  let helical = a.num_or("helical", 0.0);
  // The centre distance grows with the total shift, so invert that.
  let unshifted = (t1 + t2) / 2.0 * m / helical.to_radians().cos();
  Ok(LuaValue::Number((desired - unshifted) / m))
}

fn bevel_pitch_angle(_lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let teeth = a.need_num("teeth")?;
  let mate = a.need_num("mate_teeth")?;
  let drive = a.num_or("drive_angle", 90.0);
  Ok(LuaValue::Number(
    (drive.to_radians().sin() / ((mate / teeth) + drive.to_radians().cos()))
      .atan()
      .to_degrees(),
  ))
}

/// The centre distance two meshing gears sit at.
fn gear_dist(_lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let circ_pitch = circ_pitch_of(a)?;
  let t1 = a.need_num("teeth1")?;
  let t2 = a.num_or("teeth2", 0.0);
  let helical = a.num_or("helical", 0.0);
  let m = module_of(circ_pitch);
  let shift1 = auto_profile_shift(
    t1,
    a.num_or("pressure_angle", 20.0),
    a.num("profile_shift1"),
  );
  let shift2 = if t2 > 0.0 {
    auto_profile_shift(
      t2,
      a.num_or("pressure_angle", 20.0),
      a.num("profile_shift2"),
    )
  } else {
    0.0
  };
  // A rack has no second pitch circle, so only the first one counts.
  let base = if t2 > 0.0 {
    (t1 + t2) / 2.0 * m / helical.to_radians().cos()
  } else {
    t1 / 2.0 * m / helical.to_radians().cos()
  };
  Ok(LuaValue::Number(base + (shift1 + shift2) * m))
}

fn gear_dist_skew(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  gear_dist(lua, a)
}

fn gear_skew_angle(_lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  // Two crossed helical gears mesh when their helix angles add up to the
  // angle between their shafts.
  let h1 = a.num_or("helical1", 0.0);
  let h2 = a.num_or("helical2", 0.0);
  Ok(LuaValue::Number(h1 + h2))
}

fn gear_shorten(_lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let circ_pitch = circ_pitch_of(a)?;
  let t1 = a.need_num("teeth1")?;
  let t2 = a.num_or("teeth2", 0.0);
  let m = module_of(circ_pitch);
  let helical = a.num_or("helical", 0.0);
  let pa = a.num_or("pressure_angle", 20.0);
  let shift1 = auto_profile_shift(t1, pa, a.num("profile_shift1"));
  let shift2 = auto_profile_shift(t2.max(1.0), pa, a.num("profile_shift2"));
  // Shifting both gears outward would make the tips foul, so the addendum
  // is shortened by however much the centres moved less than the shift.
  let want = a.num("dist").unwrap_or(
    (t1 + t2) / 2.0 * m / helical.to_radians().cos() + (shift1 + shift2) * m,
  );
  let unshifted = (t1 + t2) / 2.0 * m / helical.to_radians().cos();
  Ok(LuaValue::Number(
    ((shift1 + shift2) - (want - unshifted) / m).max(0.0),
  ))
}

fn gear_shorten_skew(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  gear_shorten(lua, a)
}

fn worm_dist(_lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let d = a.need_num("d")?;
  let teeth = a.need_num("teeth")?;
  let circ_pitch = circ_pitch_of(a)?;
  let m = module_of(circ_pitch);
  let shift = a.num_or("profile_shift", 0.0);
  // Half the worm's diameter plus the gear's pitch radius.
  Ok(LuaValue::Number(d / 2.0 + teeth * m / 2.0 + shift * m))
}

fn worm_gear_thickness(_lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let worm_diam = a.need_num("worm_diam")?;
  let arc = a.num_or("worm_arc", 45.0);
  let crowning = a.num_or("crowning", 0.1);
  // The gear wraps the worm through `worm_arc`, so its face is as wide as
  // that arc's chord, plus the crowning.
  Ok(LuaValue::Number(
    worm_diam * (arc / 2.0).to_radians().sin() * 2.0 + crowning * 2.0,
  ))
}

// ---------------------------------------------------------------------------
// The gears themselves
// ---------------------------------------------------------------------------

fn tooth_steps(a: &Args) -> usize {
  a.int("fn").map(|n| (n as usize / 8).max(4)).unwrap_or(8)
}

fn spur_gear2d(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let spec = GearSpec::read(a)?;
  let path = spec.outline(
    tooth_steps(a),
    a.int("hide").unwrap_or(0).max(0) as usize,
    a.num_or("gear_spin", 0.0),
  );
  let shaft = a.num_or("shaft_diam", 0.0);
  let outline = crate::bosl::shapes2d::path_node(&path);
  let node = if shaft > 0.0 {
    ScadNode::Difference(vec![
      outline,
      ScadNode::Circle {
        r: (shaft / 2.0) as f32,
        segments: a.segments(shaft / 2.0),
      },
    ])
  } else {
    outline
  };
  let attachable = Attachable::new(Geom::Ellipse {
    r: [spec.outer_radius(), spec.outer_radius()],
  });
  as_sketch(lua, "spur_gear2d", reorient(node, a, &attachable)?)
}

fn spur_gear(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let spec = GearSpec::read(a)?;
  let thickness = a
    .num("thickness")
    .or_else(|| a.num("h"))
    .or_else(|| a.num("height"))
    .unwrap_or(10.0);
  let path = spec.outline(
    tooth_steps(a),
    a.int("hide").unwrap_or(0).max(0) as usize,
    a.num_or("gear_spin", 0.0),
  );

  // A helical gear is the same outline twisted as it is extruded, by however
  // much the helix angle carries it over the face width.
  let twist = if spec.helical.abs() > 1e-9 {
    thickness * spec.helical.to_radians().tan() / spec.pitch_radius() * 180.0
      / PI
  } else {
    0.0
  };
  let slices = if twist.abs() > 1e-9 {
    ((twist.abs() / 5.0).ceil() as u32).max(2)
  } else {
    1
  };
  let node = ScadNode::LinearExtrude {
    height: thickness as f32,
    center: true,
    twist: twist as f32,
    slices,
    scale: 1.0,
    child: Box::new(crate::bosl::shapes2d::path_node(&path)),
  };
  let shaft = a.num_or("shaft_diam", 0.0);
  let node = if shaft > 0.0 {
    ScadNode::Difference(vec![
      node,
      ScadNode::Cylinder {
        r1: (shaft / 2.0) as f32,
        r2: (shaft / 2.0) as f32,
        h: (thickness + 1.0) as f32,
        segments: a.segments(shaft / 2.0),
        center: true,
      },
    ])
  } else {
    node
  };

  let attachable = Attachable::new(Geom::Conoid {
    r1: [spec.outer_radius(), spec.outer_radius()],
    r2: [spec.outer_radius(), spec.outer_radius()],
    l: thickness,
    shift: [0.0, 0.0],
    axis: [0.0, 0.0, 1.0],
  });
  as_geometry(lua, "spur_gear", reorient(node, a, &attachable)?)
}

/// A ring gear: teeth cut into the inside of a rim.
fn ring_gear(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let mut spec = GearSpec::read(a)?;
  spec.internal = true;
  let thickness = a.num("thickness").or_else(|| a.num("h")).unwrap_or(10.0);
  let backing = a.num_or("backing", module_of(spec.circ_pitch) * 3.0);
  let path = spec.outline(tooth_steps(a), 0, a.num_or("gear_spin", 0.0));
  let rim = spec.outer_radius() + backing;

  let node = ScadNode::Difference(vec![
    ScadNode::Cylinder {
      r1: rim as f32,
      r2: rim as f32,
      h: thickness as f32,
      segments: a.segments(rim),
      center: true,
    },
    ScadNode::LinearExtrude {
      height: (thickness + 1.0) as f32,
      center: true,
      twist: 0.0,
      slices: 1,
      scale: 1.0,
      child: Box::new(crate::bosl::shapes2d::path_node(&path)),
    },
  ]);
  let attachable = Attachable::new(Geom::Conoid {
    r1: [rim, rim],
    r2: [rim, rim],
    l: thickness,
    shift: [0.0, 0.0],
    axis: [0.0, 0.0, 1.0],
  });
  as_geometry(lua, "ring_gear", reorient(node, a, &attachable)?)
}

fn ring_gear2d(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let mut spec = GearSpec::read(a)?;
  spec.internal = true;
  let backing = a.num_or("backing", module_of(spec.circ_pitch) * 3.0);
  let path = spec.outline(tooth_steps(a), 0, a.num_or("gear_spin", 0.0));
  let rim = spec.outer_radius() + backing;
  let node = ScadNode::Difference(vec![
    ScadNode::Circle {
      r: rim as f32,
      segments: a.segments(rim),
    },
    crate::bosl::shapes2d::path_node(&path),
  ]);
  let attachable = Attachable::new(Geom::Ellipse { r: [rim, rim] });
  as_sketch(lua, "ring_gear2d", reorient(node, a, &attachable)?)
}

/// A rack: a gear of infinite radius, so its teeth are straight-sided.
fn rack_profile(a: &Args) -> LuaResult<(Vec<[f64; 2]>, f64, f64)> {
  let circ_pitch = circ_pitch_of(a)?;
  let teeth = a.need_num("teeth")? as usize;
  let m = module_of(circ_pitch);
  let pa = a.num_or("pressure_angle", 20.0);
  let clearance = a.num("clearance").unwrap_or(0.25 * m);
  let backlash = a.num_or("backlash", 0.0);
  let bottom = a.num_or("bottom", m * 2.0 + clearance);

  let add = m;
  let ded = m + clearance;
  // On a rack the flanks are simply straight lines at the pressure angle.
  let lean = pa.to_radians().tan();
  let half_top = circ_pitch / 4.0 - add * lean - backlash / 2.0;
  let half_bot = circ_pitch / 4.0 + ded * lean - backlash / 2.0;

  let length = circ_pitch * teeth as f64;
  let mut path: Vec<[f64; 2]> = vec![[-length / 2.0, -bottom + ded]];
  for i in 0..teeth {
    let centre = -length / 2.0 + circ_pitch * (i as f64 + 0.5);
    path.push([centre - half_bot, -ded]);
    path.push([centre - half_top, add]);
    path.push([centre + half_top, add]);
    path.push([centre + half_bot, -ded]);
  }
  path.push([length / 2.0, -bottom + ded]);
  path.push([length / 2.0, -bottom]);
  path.push([-length / 2.0, -bottom]);
  Ok((path, length, bottom))
}

fn rack2d(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let (path, length, bottom) = rack_profile(a)?;
  let attachable = Attachable::new(Geom::Trapezoid {
    size: [length, bottom],
    size2: length,
    shift: 0.0,
  });
  as_sketch(
    lua,
    "rack2d",
    reorient(crate::bosl::shapes2d::path_node(&path), a, &attachable)?,
  )
}

fn rack(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let (path, length, bottom) = rack_profile(a)?;
  let width = a
    .num("width")
    .or_else(|| a.num("thickness"))
    .or_else(|| a.num("h"))
    .unwrap_or(10.0);
  let node = ScadNode::LinearExtrude {
    height: width as f32,
    center: true,
    twist: 0.0,
    slices: 1,
    scale: 1.0,
    child: Box::new(crate::bosl::shapes2d::path_node(&path)),
  };
  // A rack lies with its teeth up and its length along X.
  let node = crate::bosl::attach::transform(node, Mat4::xrot(90.0));
  let attachable = Attachable::new(Geom::Prismoid {
    size: [length, width, bottom],
    size2: [length, width],
    shift: [0.0, 0.0],
    axis: [0.0, 0.0, 1.0],
  });
  as_geometry(lua, "rack", reorient(node, a, &attachable)?)
}

/// A bevel gear: a spur gear's profile projected onto a cone.
fn bevel_gear(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let spec = GearSpec::read(a)?;
  let mate = a.num_or("mate_teeth", spec.teeth);
  let drive = a.num_or("spiral", 0.0);
  let _ = drive;
  let face = a.num("face_width").unwrap_or(spec.pitch_radius() / 3.0);
  let pitch_angle = (a.num_or("drive_angle", 90.0).to_radians().sin()
    / ((mate / spec.teeth) + a.num_or("drive_angle", 90.0).to_radians().cos()))
  .atan();

  let path = spec.outline(tooth_steps(a), 0, a.num_or("gear_spin", 0.0));
  let pr = spec.pitch_radius();
  // The cone distance: how far the pitch circle is from the cone's apex.
  let cone = pr / pitch_angle.sin();
  let inner = ((cone - face) / cone).max(0.05);

  // The profile shrinks toward the apex and rises as it goes. The outline
  // has to wind counter-clockwise for the loft to face outward.
  let path = crate::bosl::vnf::ccw(path);
  let steps = 8usize;
  let rows: Vec<Vec<[f64; 3]>> = (0..=steps)
    .map(|i| {
      let t = i as f64 / steps as f64;
      let k = 1.0 - (1.0 - inner) * t;
      let z = face * t * pitch_angle.cos();
      path.iter().map(|p| [p[0] * k, p[1] * k, z]).collect()
    })
    .collect();
  let vnf = Vnf::vertex_array(&rows, Caps::BOTH, true, false);
  let attachable = Attachable::new(Geom::Conoid {
    r1: [spec.outer_radius(), spec.outer_radius()],
    r2: [spec.outer_radius() * inner, spec.outer_radius() * inner],
    l: face * pitch_angle.cos(),
    shift: [0.0, 0.0],
    axis: [0.0, 0.0, 1.0],
  });
  as_geometry(lua, "bevel_gear", reorient(vnf.to_node(), a, &attachable)?)
}

fn crown_gear(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  // A crown gear's teeth stand on the face of a disc rather than its rim.
  let spec = GearSpec::read(a)?;
  let backing = a.num_or("backing", module_of(spec.circ_pitch) * 3.0);
  let face = a
    .num("face_width")
    .unwrap_or(module_of(spec.circ_pitch) * 4.0);
  let pr = spec.pitch_radius();
  let m = module_of(spec.circ_pitch);

  let disc = ScadNode::Cylinder {
    r1: (pr + face / 2.0) as f32,
    r2: (pr + face / 2.0) as f32,
    h: backing as f32,
    segments: a.segments(pr),
    center: true,
  };
  // Each tooth is a wedge standing on the face, tapering toward the centre.
  let n = spec.teeth as usize;
  let mut teeth: Vec<ScadNode> = Vec::new();
  for i in 0..n {
    let ang = 360.0 * i as f64 / n as f64;
    let tooth = ScadNode::Polyhedron {
      points: vec![
        [
          (pr - face / 2.0) as f32,
          (-m * 0.4) as f32,
          (backing / 2.0) as f32,
        ],
        [
          (pr + face / 2.0) as f32,
          (-m * 0.6) as f32,
          (backing / 2.0) as f32,
        ],
        [
          (pr + face / 2.0) as f32,
          (m * 0.6) as f32,
          (backing / 2.0) as f32,
        ],
        [
          (pr - face / 2.0) as f32,
          (m * 0.4) as f32,
          (backing / 2.0) as f32,
        ],
        [(pr - face / 2.0) as f32, 0.0, (backing / 2.0 + m) as f32],
        [(pr + face / 2.0) as f32, 0.0, (backing / 2.0 + m) as f32],
      ],
      faces: vec![
        vec![3, 2, 1, 0],
        vec![0, 1, 5, 4],
        vec![2, 3, 4, 5],
        vec![1, 2, 5],
        vec![3, 0, 4],
      ],
    };
    teeth.push(crate::bosl::attach::transform(tooth, Mat4::zrot(ang)));
  }
  let node =
    ScadNode::Union(std::iter::once(disc).chain(teeth).collect::<Vec<_>>());
  let attachable = Attachable::new(Geom::Conoid {
    r1: [pr + face / 2.0, pr + face / 2.0],
    r2: [pr + face / 2.0, pr + face / 2.0],
    l: backing,
    shift: [0.0, 0.0],
    axis: [0.0, 0.0, 1.0],
  });
  as_geometry(lua, "crown_gear", reorient(node, a, &attachable)?)
}

/// A worm: a screw whose thread meshes with a gear.
fn worm(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let circ_pitch = circ_pitch_of(a)?;
  let d = a.need_num("d")?;
  let starts = a.int("starts").unwrap_or(1).max(1) as usize;
  let l = a.num("l").or_else(|| a.num("length")).unwrap_or(d * 2.0);
  let pa = a.num_or("pressure_angle", 20.0);
  let m = module_of(circ_pitch);
  let left_handed = a.bool_or("left_handed", false);

  // The worm's thread is the rack's tooth form wrapped round a cylinder.
  let add = m;
  let ded = m * 1.25;
  let lean = pa.to_radians().tan();
  let pitch = circ_pitch;
  let half_top = 0.25 - add * lean / pitch;
  let half_bot = 0.25 + ded * lean / pitch;
  let profile = vec![
    [-half_bot, -ded / pitch],
    [-half_top, add / pitch],
    [half_top, add / pitch],
    [half_bot, -ded / pitch],
  ];

  let r = d / 2.0;
  let facets = a.segments(r);
  let turns = l / (pitch * starts as f64);
  let steps = ((turns.abs() * facets as f64).ceil() as usize).max(12);
  let mut parts: Vec<ScadNode> = Vec::new();
  for start in 0..starts {
    let phase = 360.0 * start as f64 / starts as f64;
    let rows: Vec<Vec<[f64; 3]>> = (0..=steps)
      .map(|i| {
        let u = i as f64 / steps as f64;
        let ang =
          phase + 360.0 * turns * u * if left_handed { -1.0 } else { 1.0 };
        let z = -l / 2.0 + l * u;
        let (s, c) = ang.to_radians().sin_cos();
        let ordered: Vec<[f64; 2]> = if left_handed {
          profile.clone()
        } else {
          profile.iter().rev().copied().collect()
        };
        ordered
          .iter()
          .map(|p| {
            let radius = r + p[1] * pitch;
            [radius * c, radius * s, z + p[0] * pitch * starts as f64]
          })
          .collect()
      })
      .collect();
    parts.push(Vnf::vertex_array(&rows, Caps::BOTH, true, false).to_node());
  }
  parts.push(ScadNode::Cylinder {
    r1: (r - ded) as f32,
    r2: (r - ded) as f32,
    h: l as f32,
    segments: facets,
    center: true,
  });
  let node = ScadNode::Intersection(vec![
    ScadNode::Union(parts),
    ScadNode::Cylinder {
      r1: (r * 2.0) as f32,
      r2: (r * 2.0) as f32,
      h: l as f32,
      segments: 8,
      center: true,
    },
  ]);
  let attachable = Attachable::new(Geom::Conoid {
    r1: [r, r],
    r2: [r, r],
    l,
    shift: [0.0, 0.0],
    axis: [0.0, 0.0, 1.0],
  });
  as_geometry(lua, "worm", reorient(node, a, &attachable)?)
}

fn enveloping_worm(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  worm(lua, a)
}

/// A worm gear: a spur gear whose face is hollowed to wrap the worm.
fn worm_gear(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let spec = GearSpec::read(a)?;
  let worm_diam = a.need_num("worm_diam")?;
  let arc = a.num_or("worm_arc", 45.0);
  let crowning = a.num_or("crowning", 0.1);
  let thickness =
    worm_diam * (arc / 2.0).to_radians().sin() * 2.0 + crowning * 2.0;

  let path = spec.outline(tooth_steps(a), 0, a.num_or("gear_spin", 0.0));
  let body = ScadNode::LinearExtrude {
    height: thickness as f32,
    center: true,
    twist: 0.0,
    slices: 1,
    scale: 1.0,
    child: Box::new(crate::bosl::shapes2d::path_node(&path)),
  };
  // The throat is the worm's own cylinder, swept round the gear's axis.
  let dist = worm_diam / 2.0 + spec.pitch_radius();
  let throat = ScadNode::Union(
    (0..48)
      .map(|i| {
        let ang = 360.0 * i as f64 / 48.0;
        crate::bosl::attach::transform(
          ScadNode::Cylinder {
            r1: (worm_diam / 2.0) as f32,
            r2: (worm_diam / 2.0) as f32,
            h: worm_diam as f32,
            segments: 16,
            center: true,
          },
          Mat4::zrot(ang)
            .mul(&Mat4::translate([dist, 0.0, 0.0]))
            .mul(&Mat4::xrot(90.0)),
        )
      })
      .collect::<Vec<_>>(),
  );
  let node = ScadNode::Difference(vec![body, throat]);
  let attachable = Attachable::new(Geom::Conoid {
    r1: [spec.outer_radius(), spec.outer_radius()],
    r2: [spec.outer_radius(), spec.outer_radius()],
    l: thickness,
    shift: [0.0, 0.0],
    axis: [0.0, 0.0, 1.0],
  });
  as_geometry(lua, "worm_gear", reorient(node, a, &attachable)?)
}

/// A planetary set: the sun, its planets and the ring around them.
fn planetary_gears(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let circ_pitch = circ_pitch_of(a)?;
  let sun_teeth = a.need_num("sun_teeth")?;
  let ring_teeth = a.need_num("ring_teeth")?;
  let n = a.num_or("n", 3.0).max(1.0) as usize;
  let thickness = a.num("thickness").or_else(|| a.num("h")).unwrap_or(10.0);
  let planet_teeth = (ring_teeth - sun_teeth) / 2.0;
  if planet_teeth <= 3.0 {
    return a.err("the ring and sun tooth counts leave no room for planets");
  }
  let m = module_of(circ_pitch);
  let orbit = (sun_teeth + planet_teeth) * m / 2.0;

  // Each part is an ordinary gear; only their placement is special.
  let gear_at = |teeth: f64, internal: bool| -> LuaResult<ScadNode> {
    let spec = GearSpec {
      circ_pitch,
      teeth,
      pressure_angle: a.num_or("pressure_angle", 20.0),
      clearance: a.num("clearance"),
      backlash: a.num_or("backlash", 0.0),
      helical: a.num_or("helical", 0.0),
      profile_shift: auto_profile_shift(
        teeth,
        a.num_or("pressure_angle", 20.0),
        a.num("profile_shift"),
      ),
      shorten: 0.0,
      internal,
    };
    let path = spec.outline(tooth_steps(a), 0, 0.0);
    let solid = ScadNode::LinearExtrude {
      height: thickness as f32,
      center: true,
      twist: 0.0,
      slices: 1,
      scale: 1.0,
      child: Box::new(crate::bosl::shapes2d::path_node(&path)),
    };
    Ok(if internal {
      let rim = spec.outer_radius() + m * 3.0;
      ScadNode::Difference(vec![
        ScadNode::Cylinder {
          r1: rim as f32,
          r2: rim as f32,
          h: thickness as f32,
          segments: a.segments(rim),
          center: true,
        },
        solid,
      ])
    } else {
      solid
    })
  };

  let mut parts = vec![gear_at(sun_teeth, false)?, gear_at(ring_teeth, true)?];
  let planet = gear_at(planet_teeth, false)?;
  for i in 0..n {
    let ang = 360.0 * i as f64 / n as f64;
    parts.push(crate::bosl::attach::transform(
      planet.clone(),
      Mat4::zrot(ang).mul(&Mat4::translate([orbit, 0.0, 0.0])),
    ));
  }
  let rim = ring_teeth * m / 2.0 + m * 4.0;
  let attachable = Attachable::new(Geom::Conoid {
    r1: [rim, rim],
    r2: [rim, rim],
    l: thickness,
    shift: [0.0, 0.0],
    axis: [0.0, 0.0, 1.0],
  });
  as_geometry(
    lua,
    "planetary_gears",
    reorient(ScadNode::Union(parts), a, &attachable)?,
  )
}

// ---------------------------------------------------------------------------
// Registration
// ---------------------------------------------------------------------------

const SIZE_PARAMS: &[&str] = &["circ_pitch", "mod", "pitch", "diam_pitch"];

const GEAR_PARAMS: &[&str] = &[
  "circ_pitch",
  "teeth",
  "thickness",
  "shaft_diam",
  "hide",
  "pressure_angle",
  "clearance",
  "backlash",
  "helical",
  "slices",
  "internal",
  "profile_shift",
  "shorten",
  "mod",
  "pitch",
  "diam_pitch",
  "gear_spin",
  "atype",
  "h",
  "height",
  "anchor",
  "spin",
  "orient",
  "fn",
  "backing",
  "face_width",
  "mate_teeth",
  "drive_angle",
  "spiral",
  "worm_diam",
  "worm_arc",
  "crowning",
  "starts",
  "d",
  "l",
  "length",
  "left_handed",
  "width",
  "bottom",
  "sun_teeth",
  "ring_teeth",
  "n",
];

pub fn register(lua: &Lua, bosl: &mlua::Table) -> LuaResult<()> {
  use crate::bosl::threading::register_one;

  register_one(lua, bosl, "circular_pitch", SIZE_PARAMS, circular_pitch)?;
  register_one(lua, bosl, "diametral_pitch", SIZE_PARAMS, diametral_pitch)?;
  register_one(lua, bosl, "module_value", SIZE_PARAMS, module_value)?;
  register_one(
    lua,
    bosl,
    "pitch_radius",
    &[
      "circ_pitch",
      "teeth",
      "helical",
      "mod",
      "diam_pitch",
      "pitch",
    ],
    pitch_radius,
  )?;
  register_one(lua, bosl, "outer_radius", GEAR_PARAMS, outer_radius)?;
  register_one(lua, bosl, "root_radius", GEAR_PARAMS, root_radius)?;
  register_one(
    lua,
    bosl,
    "auto_profile_shift",
    &[
      "teeth",
      "pressure_angle",
      "helical",
      "min_teeth",
      "profile_shift",
      "get_min",
    ],
    auto_profile_shift_fn,
  )?;
  register_one(
    lua,
    bosl,
    "get_profile_shift",
    &[
      "desired",
      "teeth1",
      "teeth2",
      "helical",
      "pressure_angle",
      "mod",
      "diam_pitch",
      "circ_pitch",
    ],
    get_profile_shift,
  )?;
  register_one(
    lua,
    bosl,
    "bevel_pitch_angle",
    &["teeth", "mate_teeth", "drive_angle"],
    bevel_pitch_angle,
  )?;

  const MESH_PARAMS: &[&str] = &[
    "teeth1",
    "teeth2",
    "helical",
    "profile_shift1",
    "profile_shift2",
    "pressure_angle",
    "mod",
    "circ_pitch",
    "diam_pitch",
    "backlash",
    "dist",
    "helical1",
    "helical2",
  ];
  register_one(lua, bosl, "gear_dist", MESH_PARAMS, gear_dist)?;
  register_one(lua, bosl, "gear_dist_skew", MESH_PARAMS, gear_dist_skew)?;
  register_one(lua, bosl, "gear_skew_angle", MESH_PARAMS, gear_skew_angle)?;
  register_one(lua, bosl, "gear_shorten", MESH_PARAMS, gear_shorten)?;
  register_one(
    lua,
    bosl,
    "gear_shorten_skew",
    MESH_PARAMS,
    gear_shorten_skew,
  )?;
  register_one(
    lua,
    bosl,
    "worm_dist",
    &[
      "d",
      "starts",
      "teeth",
      "mod",
      "profile_shift",
      "diam_pitch",
      "circ_pitch",
      "pressure_angle",
      "backlash",
    ],
    worm_dist,
  )?;
  register_one(
    lua,
    bosl,
    "worm_gear_thickness",
    &[
      "circ_pitch",
      "teeth",
      "worm_diam",
      "worm_arc",
      "pressure_angle",
      "crowning",
      "clearance",
      "diam_pitch",
      "mod",
      "pitch",
    ],
    worm_gear_thickness,
  )?;

  for (name, f) in [
    (
      "spur_gear",
      spur_gear as fn(&Lua, &Args) -> LuaResult<LuaValue>,
    ),
    ("spur_gear2d", spur_gear2d),
    ("ring_gear", ring_gear),
    ("ring_gear2d", ring_gear2d),
    ("rack", rack),
    ("rack2d", rack2d),
    ("bevel_gear", bevel_gear),
    ("crown_gear", crown_gear),
    ("worm", worm),
    ("enveloping_worm", enveloping_worm),
    ("worm_gear", worm_gear),
    ("planetary_gears", planetary_gears),
  ] {
    register_one(lua, bosl, name, GEAR_PARAMS, f)?;
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
  fn the_four_ways_of_naming_a_size_agree() {
    // A module of 2 is a circular pitch of 2 pi.
    let cp: f64 = eval("return bosl.circular_pitch({mod = 2})");
    assert!((cp - 2.0 * std::f64::consts::PI).abs() < 1e-9, "{cp}");
    let m: f64 = eval("return bosl.module_value({circ_pitch = 2 * math.pi})");
    assert!((m - 2.0).abs() < 1e-9, "{m}");
    // Diametral pitch is teeth per inch of pitch diameter.
    let dp: f64 = eval("return bosl.diametral_pitch({mod = 25.4})");
    assert!((dp - 1.0).abs() < 1e-9, "{dp}");
  }

  #[test]
  fn the_pitch_radius_is_half_the_module_times_the_teeth() {
    let r: f64 = eval("return bosl.pitch_radius({mod = 2, teeth = 20})");
    assert!((r - 20.0).abs() < 1e-9, "{r}");
  }

  #[test]
  fn the_tip_and_root_sit_a_module_either_side_of_the_pitch_circle() {
    // With enough teeth there is no profile shift to complicate it.
    let outer: f64 = eval("return bosl.outer_radius({mod = 2, teeth = 30})");
    let root: f64 = eval("return bosl.root_radius({mod = 2, teeth = 30})");
    assert!((outer - 32.0).abs() < 1e-6, "{outer}");
    // The root also allows a quarter module of clearance.
    assert!((root - (30.0 - 2.0 - 0.5)).abs() < 1e-6, "{root}");
  }

  #[test]
  fn a_small_gear_is_shifted_to_avoid_undercutting() {
    let shift: f64 = eval("return bosl.auto_profile_shift({teeth = 10})");
    assert!(shift > 0.0, "{shift}");
    let none: f64 = eval("return bosl.auto_profile_shift({teeth = 30})");
    assert_eq!(none, 0.0);
  }

  #[test]
  fn meshing_gears_sit_at_the_sum_of_their_pitch_radii() {
    let d: f64 =
      eval("return bosl.gear_dist({mod = 2, teeth1 = 20, teeth2 = 30})");
    assert!((d - 50.0).abs() < 1e-6, "{d}");
  }

  #[test]
  fn a_spur_gear_fits_inside_its_tip_circle() {
    let (v, (lo, hi)) =
      measure("render(bosl.spur_gear({mod = 2, teeth = 20, thickness = 5}))");
    assert!(v > 0.0);
    // The tip radius of a 20-tooth module-2 gear is 22.
    assert!(hi[0] <= 22.1 && hi[0] > 20.0, "{hi:?}");
    assert!(lo[0] >= -22.1 && lo[0] < -20.0, "{lo:?}");
    assert!((hi[2] - 2.5).abs() < 1e-3, "{hi:?}");
  }

  #[test]
  fn a_gear_has_less_metal_than_its_tip_circle_and_more_than_its_root() {
    let (v, _) =
      measure("render(bosl.spur_gear({mod = 2, teeth = 20, thickness = 5}))");
    let tip = std::f64::consts::PI * 22.0f64.powi(2) * 5.0;
    let root = std::f64::consts::PI * 17.5f64.powi(2) * 5.0;
    assert!(v < tip && v > root, "{v} not between {root} and {tip}");
  }

  #[test]
  fn a_shaft_hole_removes_material() {
    let (plain, _) =
      measure("render(bosl.spur_gear({mod = 2, teeth = 20, thickness = 5}))");
    let (bored, _) = measure(
      "render(bosl.spur_gear({mod = 2, teeth = 20, thickness = 5,
                              shaft_diam = 8, fn = 128}))",
    );
    let hole = std::f64::consts::PI * 16.0 * 5.0;
    assert!(
      (plain - bored - hole).abs() / hole < 0.02,
      "{plain} {bored}"
    );
  }

  #[test]
  fn more_teeth_make_a_larger_gear() {
    let (_, (_, small)) =
      measure("render(bosl.spur_gear({mod = 2, teeth = 12, thickness = 5}))");
    let (_, (_, big)) =
      measure("render(bosl.spur_gear({mod = 2, teeth = 40, thickness = 5}))");
    assert!(big[0] > small[0] * 2.5, "{small:?} {big:?}");
  }

  #[test]
  fn a_gear_outline_is_a_sketch_that_extrudes() {
    let (v, _) = measure(
      "render((bosl.spur_gear2d({mod = 2, teeth = 20})):linear_extrude(5))",
    );
    assert!(v > 0.0);
  }

  #[test]
  fn a_helical_gear_is_twisted_along_its_face() {
    let (straight, _) =
      measure("render(bosl.spur_gear({mod = 2, teeth = 20, thickness = 10}))");
    let (helical, _) = measure(
      "render(bosl.spur_gear({mod = 2, teeth = 20, thickness = 10,
                              helical = 25}))",
    );
    // A helical gear of the same tooth count is a little larger, since its
    // pitch radius grows with the helix angle.
    assert!(helical > straight, "{helical} vs {straight}");
  }

  #[test]
  fn a_ring_gear_is_a_rim_with_teeth_inside_it() {
    let (v, (_, hi)) =
      measure("render(bosl.ring_gear({mod = 2, teeth = 30, thickness = 5}))");
    assert!(v > 0.0);
    // The rim reaches past the tip circle by the backing.
    assert!(hi[0] > 30.0, "{hi:?}");
  }

  #[test]
  fn a_rack_is_as_long_as_its_teeth_make_it() {
    let (v, (lo, hi)) =
      measure("render(bosl.rack({mod = 2, teeth = 10, width = 5}))");
    assert!(v > 0.0);
    let length = 10.0 * 2.0 * std::f64::consts::PI;
    let span = (hi[0] - lo[0]) as f64;
    assert!((span - length).abs() < 0.5, "{span} vs {length}");
  }

  #[test]
  fn a_bevel_gear_tapers_toward_its_apex() {
    let (v, (lo, hi)) = measure(
      "render(bosl.bevel_gear({mod = 2, teeth = 20, mate_teeth = 20}))",
    );
    assert!(v > 0.0);
    assert!(hi[2] > lo[2], "{lo:?} {hi:?}");
  }

  #[test]
  fn a_worm_is_a_threaded_cylinder() {
    let (v, (lo, hi)) = measure("render(bosl.worm({mod = 2, d = 20, l = 30}))");
    assert!(v > 0.0);
    let span = (hi[2] - lo[2]) as f64;
    assert!((span - 30.0).abs() < 0.5, "{span}");
    // `d` is the pitch diameter, so the crests stand a module proud of it.
    assert!((hi[0] as f64 - 12.0).abs() < 0.2, "{hi:?}");
  }

  #[test]
  fn a_worm_gear_is_hollowed_where_the_worm_runs() {
    let (v, _) =
      measure("render(bosl.worm_gear({mod = 2, teeth = 30, worm_diam = 20}))");
    assert!(v > 0.0);
    let thickness: f64 = eval(
      "return bosl.worm_gear_thickness({mod = 2, teeth = 30,
                                        worm_diam = 20})",
    );
    assert!(thickness > 0.0, "{thickness}");
  }

  #[test]
  fn a_planetary_set_holds_a_sun_a_ring_and_its_planets() {
    let (v, _) = measure(
      "render(bosl.planetary_gears({mod = 1, sun_teeth = 20,
                                    ring_teeth = 60, n = 3,
                                    thickness = 5}))",
    );
    assert!(v > 0.0);
  }

  #[test]
  fn a_crown_gear_has_teeth_on_its_face() {
    let (v, _) = measure("render(bosl.crown_gear({mod = 2, teeth = 20}))");
    assert!(v > 0.0);
  }

  #[test]
  fn a_bevel_pitch_angle_splits_the_drive_angle() {
    // Two equal gears at right angles each take half the angle.
    let ang: f64 =
      eval("return bosl.bevel_pitch_angle({teeth = 20, mate_teeth = 20})");
    assert!((ang - 45.0).abs() < 1e-6, "{ang}");
  }

  #[test]
  fn a_gear_needs_more_than_three_teeth() {
    let lua = Lua::new();
    register_bosl(&lua).unwrap();
    let err = lua
      .load("return bosl.spur_gear({mod = 2, teeth = 3})")
      .eval::<mlua::Value>()
      .unwrap_err()
      .to_string();
    assert!(err.contains("three teeth"), "{err}");
  }

  #[test]
  fn a_gear_with_no_size_given_is_reported() {
    let lua = Lua::new();
    register_bosl(&lua).unwrap();
    let err = lua
      .load("return bosl.spur_gear({teeth = 20})")
      .eval::<mlua::Value>()
      .unwrap_err()
      .to_string();
    assert!(err.contains("circ_pitch"), "{err}");
  }
}
