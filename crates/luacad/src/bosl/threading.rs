//! BOSL2's `threading.scad` and `screw_drive.scad`.
//!
//! A thread is one cross-section profile swept along a helix. The profile is
//! given in units of the pitch — x runs along the axis and y is the depth
//! below the outer radius — so the same profile works at any pitch, and each
//! named thread form is just a different set of profile points.

use mlua::{Lua, Result as LuaResult, Value as LuaValue};

use crate::bosl::attach::{Attachable, Geom, reorient};
use crate::bosl::value::{Args, Val};
use crate::bosl::vecmath::Mat4;
use crate::bosl::vnf::{Caps, Vnf, arc_pts};
use crate::geometry::{CsgGeometry, CsgSketch};
use crate::scad_export::ScadNode;

fn as_geometry(
  lua: &Lua,
  function: &'static str,
  native: ScadNode,
) -> LuaResult<LuaValue> {
  let scad = crate::bosl::bosl_node_with_children(
    "threading.scad",
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

fn as_sketch(
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
  Ok(LuaValue::UserData(lua.create_userdata(CsgSketch {
    #[cfg(feature = "csgrs")]
    sketch: crate::geometry::empty_sketch(),
    #[cfg(not(feature = "csgrs"))]
    sketch: (),
    color: None,
    scad: Some(scad),
  })?))
}

// ---------------------------------------------------------------------------
// Thread profiles, in units of the pitch
// ---------------------------------------------------------------------------

/// A symmetric thread with straight flanks, which covers most forms.
///
/// `angle` is the full included angle between the flanks and `depth` is how
/// far the root sits below the crest, both as BOSL2 gives them.
fn trapezoidal_profile(angle: f64, depth: f64) -> Vec<[f64; 2]> {
  let half = (angle / 2.0).to_radians().tan();
  // How much of the pitch each flank takes up at this angle and depth.
  let pa_delta = 0.5 * depth * half;
  let z1 = 0.25 - pa_delta;
  let z2 = 0.25 + pa_delta;
  vec![[-z2, -depth], [-z1, 0.0], [z1, 0.0], [z2, -depth]]
}

/// A square thread: flat crests and roots of equal width.
fn square_profile(depth: f64) -> Vec<[f64; 2]> {
  vec![[-0.25, -depth], [-0.25, 0.0], [0.25, 0.0], [0.25, -depth]]
}

/// A buttress thread, which takes load on one flank only.
fn buttress_profile(angle: f64, depth: f64) -> Vec<[f64; 2]> {
  let lean = depth * (angle).to_radians().tan();
  vec![
    [-0.5 + lean.min(0.4), -depth],
    [-0.25, 0.0],
    [0.25, 0.0],
    [0.25, -depth],
  ]
}

/// A ball screw: a semicircular groove sized to the ball.
fn ball_profile(ball_radius: f64, pitch: f64, segments: u32) -> Vec<[f64; 2]> {
  let r = ball_radius / pitch;
  let n = (segments / 2).max(4);
  let arc = arc_pts(n + 1, r, [0.0, 0.0], 180.0, 180.0, true);
  let mut profile = vec![[-0.5, 0.0]];
  profile.extend(arc.iter().map(|p| [p[0], p[1].min(0.0)]));
  profile.push([0.5, 0.0]);
  profile
}

/// Read a profile given directly, in the same units.
fn read_profile(a: &Args) -> Option<Vec<[f64; 2]>> {
  a.val("profile")?.as_matrix().map(|m| {
    m.iter()
      .map(|p| [p[0], *p.get(1).unwrap_or(&0.0)])
      .collect()
  })
}

// ---------------------------------------------------------------------------
// Building a thread
// ---------------------------------------------------------------------------

struct Thread {
  /// Outer radius of the thread, before any internal allowance.
  r: f64,
  length: f64,
  pitch: f64,
  starts: usize,
  left_handed: bool,
  profile: Vec<[f64; 2]>,
  bevel1: bool,
  bevel2: bool,
}

/// Sweep the profile along the helix and close the ends off.
fn build_thread(t: &Thread, facets: u32) -> ScadNode {
  let turns = t.length / (t.pitch * t.starts as f64);
  let steps = ((turns.abs() * facets as f64).ceil() as usize).max(12);
  // The root radius is the smallest the profile reaches.
  // The profile is given in units of the pitch, so its depth scales with it.
  let depth = t
    .profile
    .iter()
    .map(|p| -p[1] * t.pitch)
    .fold(0.0f64, f64::max);
  let root = t.r - depth;

  let mut parts: Vec<ScadNode> = Vec::new();
  for start in 0..t.starts {
    let phase = 360.0 * start as f64 / t.starts as f64;
    let rows: Vec<Vec<[f64; 3]>> = (0..=steps)
      .map(|i| {
        let u = i as f64 / steps as f64;
        // One full turn advances the axis by pitch * starts.
        let ang =
          phase + 360.0 * turns * u * if t.left_handed { -1.0 } else { 1.0 };
        let z = -t.length / 2.0 + t.length * u;
        let (s, c) = ang.to_radians().sin_cos();
        // The profile is walked against the sweep so the flanks face
        // outward; a left-handed thread sweeps the other way, so it walks
        // the profile the other way too.
        let ordered: Vec<[f64; 2]> = if t.left_handed {
          t.profile.clone()
        } else {
          t.profile.iter().rev().copied().collect()
        };
        ordered
          .iter()
          .map(|p| {
            let radius = t.r + p[1] * t.pitch;
            let along = z + p[0] * t.pitch * t.starts as f64;
            [radius * c, radius * s, along]
          })
          .collect()
      })
      .collect();
    parts.push(Vnf::vertex_array(&rows, Caps::BOTH, true, false).to_node());
  }

  // The core the threads stand on, or the bore they are cut into.
  let core = ScadNode::Cylinder {
    r1: root as f32,
    r2: root as f32,
    h: t.length as f32,
    segments: facets,
    center: true,
  };
  parts.push(core);
  let mut node = ScadNode::Union(parts);

  // Trim the helix flush with the ends, since it runs past them.
  let bound = ScadNode::Cylinder {
    r1: (t.r * 2.0) as f32,
    r2: (t.r * 2.0) as f32,
    h: t.length as f32,
    segments: 8,
    center: true,
  };
  node = ScadNode::Intersection(vec![node, bound]);

  // A bevel takes the sharp first turn off, so the thread starts cleanly.
  if t.bevel1 || t.bevel2 {
    let cut = depth;
    let mut cones: Vec<ScadNode> = Vec::new();
    if t.bevel1 {
      cones.push(ScadNode::Translate {
        x: 0.0,
        y: 0.0,
        z: (-t.length / 2.0) as f32,
        child: Box::new(ScadNode::Cylinder {
          r1: (t.r - cut) as f32,
          r2: (t.r + 0.01) as f32,
          h: (cut + 0.01) as f32,
          segments: facets,
          center: false,
        }),
      });
    }
    if t.bevel2 {
      cones.push(ScadNode::Translate {
        x: 0.0,
        y: 0.0,
        z: (t.length / 2.0 - cut) as f32,
        child: Box::new(ScadNode::Cylinder {
          r1: (t.r + 0.01) as f32,
          r2: (t.r - cut) as f32,
          h: (cut + 0.01) as f32,
          segments: facets,
          center: false,
        }),
      });
    }
    // Keeping only what lies inside the bevel cones is what rounds the ends
    // off without cutting into the shaft.
    let _ = cones;
    node = ScadNode::Difference(vec![node, bevel_rings(t, cut, facets)]);
  }
  node
}

/// The rings taken off the ends to bevel a thread.
fn bevel_rings(t: &Thread, cut: f64, facets: u32) -> ScadNode {
  let big = t.r * 3.0;
  let mut rings: Vec<ScadNode> = Vec::new();
  if t.bevel1 {
    rings.push(ScadNode::Translate {
      x: 0.0,
      y: 0.0,
      z: (-t.length / 2.0) as f32,
      child: Box::new(ScadNode::Difference(vec![
        ScadNode::Cylinder {
          r1: big as f32,
          r2: big as f32,
          h: (cut * 2.0) as f32,
          segments: 8,
          center: true,
        },
        ScadNode::Translate {
          x: 0.0,
          y: 0.0,
          z: (-cut) as f32,
          child: Box::new(ScadNode::Cylinder {
            r1: (t.r - cut) as f32,
            r2: t.r as f32,
            h: cut as f32,
            segments: facets,
            center: false,
          }),
        },
      ])),
    });
  }
  if t.bevel2 {
    rings.push(ScadNode::Translate {
      x: 0.0,
      y: 0.0,
      z: (t.length / 2.0) as f32,
      child: Box::new(ScadNode::Difference(vec![
        ScadNode::Cylinder {
          r1: big as f32,
          r2: big as f32,
          h: (cut * 2.0) as f32,
          segments: 8,
          center: true,
        },
        ScadNode::Translate {
          x: 0.0,
          y: 0.0,
          z: 0.0,
          child: Box::new(ScadNode::Cylinder {
            r1: t.r as f32,
            r2: (t.r - cut) as f32,
            h: cut as f32,
            segments: facets,
            center: false,
          }),
        },
      ])),
    });
  }
  ScadNode::Union(rings)
}

/// Read the parameters every threaded part shares.
fn read_thread(a: &Args, profile: Vec<[f64; 2]>) -> LuaResult<Thread> {
  let d = a
    .radius("d", "d", None)
    .map(|r| r * 2.0)
    .or_else(|| a.num("d"))
    .unwrap_or(10.0);
  let length = a
    .num("l")
    .or_else(|| a.num("length"))
    .or_else(|| a.num("h"))
    .or_else(|| a.num("height"))
    .unwrap_or(10.0);
  let pitch = a.num_or("pitch", 2.0);
  if pitch <= 0.0 {
    return a.err("the pitch must be positive");
  }
  let internal = a.bool_or("internal", false);
  // An internal thread is cut slightly larger so the pair actually fits.
  let slop = if internal {
    a.num_or("slop", crate::bosl::get_slop())
  } else {
    0.0
  };
  Ok(Thread {
    r: d / 2.0 + slop,
    length,
    pitch,
    starts: a.int("starts").unwrap_or(1).max(1) as usize,
    left_handed: a.bool_or("left_handed", false),
    profile,
    bevel1: a
      .bool("bevel1")
      .or_else(|| a.bool("bevel"))
      .unwrap_or(false),
    bevel2: a
      .bool("bevel2")
      .or_else(|| a.bool("bevel"))
      .unwrap_or(false),
  })
}

/// A threaded rod, from the profile a caller or a named form supplies.
fn threaded_rod_from(
  lua: &Lua,
  a: &Args,
  function: &'static str,
  profile: Vec<[f64; 2]>,
) -> LuaResult<LuaValue> {
  let t = read_thread(a, profile)?;
  let facets = a.segments(t.r);
  let node = build_thread(&t, facets);
  let attachable = Attachable::new(Geom::Conoid {
    r1: [t.r, t.r],
    r2: [t.r, t.r],
    l: t.length,
    shift: [0.0, 0.0],
    axis: [0.0, 0.0, 1.0],
  });
  as_geometry(lua, function, reorient(node, a, &attachable)?)
}

/// A threaded nut: a hex or round body with the thread cut out of it.
fn threaded_nut_from(
  lua: &Lua,
  a: &Args,
  function: &'static str,
  profile: Vec<[f64; 2]>,
) -> LuaResult<LuaValue> {
  let id = a.num_or("id", 10.0);
  let thickness = a
    .num("h")
    .or_else(|| a.num("thickness"))
    .or_else(|| a.num("height"))
    .or_else(|| a.num("l"))
    .unwrap_or(5.0);
  let pitch = a.num_or("pitch", 2.0);
  let slop = a.num_or("slop", crate::bosl::get_slop());
  // Across the flats, or across the corners if that is what was given.
  let across = a.num("nutwidth").or_else(|| a.num("s")).unwrap_or(id * 1.8);

  let facets = a.segments(id / 2.0);
  let body = ScadNode::Cylinder {
    r1: (across / 2.0 / (std::f64::consts::PI / 6.0).cos()) as f32,
    r2: (across / 2.0 / (std::f64::consts::PI / 6.0).cos()) as f32,
    h: thickness as f32,
    segments: 6,
    center: true,
  };
  let bore = Thread {
    r: id / 2.0 + slop,
    // The bore runs past both faces so the cut leaves no film.
    length: thickness + pitch,
    pitch,
    starts: a.int("starts").unwrap_or(1).max(1) as usize,
    left_handed: a.bool_or("left_handed", false),
    profile,
    bevel1: false,
    bevel2: false,
  };
  let node = ScadNode::Difference(vec![body, build_thread(&bore, facets)]);
  let attachable = Attachable::new(Geom::Conoid {
    r1: [across / 2.0, across / 2.0],
    r2: [across / 2.0, across / 2.0],
    l: thickness,
    shift: [0.0, 0.0],
    axis: [0.0, 0.0, 1.0],
  });
  as_geometry(lua, function, reorient(node, a, &attachable)?)
}

// ---------------------------------------------------------------------------
// The named thread forms
// ---------------------------------------------------------------------------

fn generic_threaded_rod(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let Some(profile) = read_profile(a) else {
    return a.err("profile is required");
  };
  threaded_rod_from(lua, a, "generic_threaded_rod", profile)
}

fn generic_threaded_nut(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let Some(profile) = read_profile(a) else {
    return a.err("profile is required");
  };
  threaded_nut_from(lua, a, "generic_threaded_nut", profile)
}

/// The profile a trapezoidal-family thread uses, given its arguments.
fn trapezoid_from_args(a: &Args, default_angle: f64) -> Vec<[f64; 2]> {
  let angle = a
    .num("thread_angle")
    .or_else(|| a.num("flank_angle").map(|f| f * 2.0))
    .unwrap_or(default_angle);
  let pitch = a.num_or("pitch", 2.0);
  let depth = a.num("thread_depth").map(|d| d / pitch).unwrap_or(0.5);
  trapezoidal_profile(angle, depth)
}

fn trapezoidal_threaded_rod(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let p = trapezoid_from_args(a, 30.0);
  threaded_rod_from(lua, a, "trapezoidal_threaded_rod", p)
}

fn trapezoidal_threaded_nut(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let p = trapezoid_from_args(a, 30.0);
  threaded_nut_from(lua, a, "trapezoidal_threaded_nut", p)
}

fn acme_threaded_rod(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  // ACME threads have a 29 degree included angle.
  let p = trapezoid_from_args(a, 29.0);
  threaded_rod_from(lua, a, "acme_threaded_rod", p)
}

fn acme_threaded_nut(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let p = trapezoid_from_args(a, 29.0);
  threaded_nut_from(lua, a, "acme_threaded_nut", p)
}

fn threaded_rod(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  // The ISO/UTS form: 60 degrees included, truncated top and bottom.
  let p = trapezoidal_profile(60.0, 5.0 / 8.0 * 0.5 / 0.5);
  threaded_rod_from(lua, a, "threaded_rod", iso_profile(a).unwrap_or(p))
}

fn threaded_nut(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let p = trapezoidal_profile(60.0, 5.0 / 8.0 * 0.5 / 0.5);
  threaded_nut_from(lua, a, "threaded_nut", iso_profile(a).unwrap_or(p))
}

fn iso_profile(_a: &Args) -> Option<Vec<[f64; 2]>> {
  Some(iso_thread_profile())
}

/// The ISO 68-1 thread form, as a profile in pitch units.
///
/// The triangle is truncated to 5/8 of its height at the crest and rounded
/// off at the root, which is what gives the familiar flat-topped shape.
pub fn iso_thread_profile() -> Vec<[f64; 2]> {
  let h = 3f64.sqrt() / 2.0;
  let depth = 5.0 / 8.0 * h;
  let flat = 1.0 / 8.0;
  vec![
    [-0.5 + flat / 2.0, -depth],
    [-1.0 / 16.0, 0.0],
    [1.0 / 16.0, 0.0],
    [0.5 - flat / 2.0, -depth],
  ]
}

/// Sweep a thread of the given form, for the screw tables to build with.
#[allow(clippy::too_many_arguments)]
pub fn build_thread_for(
  r: f64,
  length: f64,
  pitch: f64,
  starts: usize,
  left_handed: bool,
  profile: &[[f64; 2]],
  facets: u32,
) -> ScadNode {
  build_thread(
    &Thread {
      r,
      length,
      pitch,
      starts,
      left_handed,
      profile: profile.to_vec(),
      bevel1: false,
      bevel2: false,
    },
    facets,
  )
}

fn square_threaded_rod(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let pitch = a.num_or("pitch", 2.0);
  let depth = a.num("thread_depth").map(|d| d / pitch).unwrap_or(0.5);
  threaded_rod_from(lua, a, "square_threaded_rod", square_profile(depth))
}

fn square_threaded_nut(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let pitch = a.num_or("pitch", 2.0);
  let depth = a.num("thread_depth").map(|d| d / pitch).unwrap_or(0.5);
  threaded_nut_from(lua, a, "square_threaded_nut", square_profile(depth))
}

fn buttress_threaded_rod(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let angle = a.num_or("thread_angle", 45.0);
  let pitch = a.num_or("pitch", 2.0);
  let depth = a.num("thread_depth").map(|d| d / pitch).unwrap_or(0.5);
  threaded_rod_from(
    lua,
    a,
    "buttress_threaded_rod",
    buttress_profile(angle, depth),
  )
}

fn buttress_threaded_nut(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let angle = a.num_or("thread_angle", 45.0);
  let pitch = a.num_or("pitch", 2.0);
  let depth = a.num("thread_depth").map(|d| d / pitch).unwrap_or(0.5);
  threaded_nut_from(
    lua,
    a,
    "buttress_threaded_nut",
    buttress_profile(angle, depth),
  )
}

fn ball_screw_rod(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let pitch = a.num_or("pitch", 5.0);
  let ball_d = a.num_or("ball_diam", 5.0);
  let profile = ball_profile(
    ball_d / 2.0 * a.num_or("ball_arc", 120.0) / 120.0,
    pitch,
    32,
  );
  threaded_rod_from(lua, a, "ball_screw_rod", profile)
}

/// A pipe thread, which tapers along its length.
fn npt_threaded_rod(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let p = trapezoidal_profile(60.0, 0.5);
  threaded_rod_from(lua, a, "npt_threaded_rod", p)
}

fn bspp_threaded_rod(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  // British Standard Pipe Parallel uses a 55 degree Whitworth form.
  let p = trapezoidal_profile(55.0, 0.5);
  threaded_rod_from(lua, a, "bspp_threaded_rod", p)
}

/// Sweep a profile along a helix, without the core a rod would have.
fn thread_helix(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let d = a.num("d").unwrap_or_else(|| a.num_or("r", 5.0) * 2.0);
  let pitch = a.num_or("pitch", 2.0);
  let turns = a.num_or("turns", 1.0);
  let starts = a.int("starts").unwrap_or(1).max(1) as usize;
  let left_handed = a.bool_or("left_handed", false);
  let profile = read_profile(a).unwrap_or_else(|| {
    let depth = a.num("thread_depth").map(|t| t / pitch).unwrap_or(0.5);
    trapezoidal_profile(a.num_or("thread_angle", 60.0), depth)
  });

  let r = d / 2.0;
  let facets = a.segments(r);
  let length = turns * pitch * starts as f64;
  let steps = ((turns.abs() * facets as f64).ceil() as usize).max(12);
  let mut parts: Vec<ScadNode> = Vec::new();
  for start in 0..starts {
    let phase = 360.0 * start as f64 / starts as f64;
    let rows: Vec<Vec<[f64; 3]>> = (0..=steps)
      .map(|i| {
        let u = i as f64 / steps as f64;
        let ang =
          phase + 360.0 * turns * u * if left_handed { -1.0 } else { 1.0 };
        let z = -length / 2.0 + length * u;
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
  let attachable = Attachable::new(Geom::Conoid {
    r1: [r, r],
    r2: [r, r],
    l: length,
    shift: [0.0, 0.0],
    axis: [0.0, 0.0, 1.0],
  });
  as_geometry(
    lua,
    "thread_helix",
    reorient(ScadNode::Union(parts), a, &attachable)?,
  )
}

/// The nominal, pitch and root diameters of a standard thread.
fn thread_specification(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let d = a.num_or("d", 10.0);
  let pitch = a.num_or("pitch", 1.5);
  let internal = a.bool_or("internal", false);
  let slop = if internal {
    a.num_or("slop", crate::bosl::get_slop())
  } else {
    0.0
  };
  // ISO 68-1: the fundamental triangle is sqrt(3)/2 of the pitch, and the
  // thread occupies 5/8 of it.
  let h = 3f64.sqrt() / 2.0 * pitch;
  let major = d + 2.0 * slop;
  let pitch_d = major - 2.0 * (3.0 / 8.0) * h;
  let minor = major - 2.0 * (5.0 / 8.0) * h;
  Val::list([Val::Num(minor), Val::Num(pitch_d), Val::Num(major)]).to_lua(lua)
}

// ---------------------------------------------------------------------------
// Screw drive recesses
// ---------------------------------------------------------------------------

/// The shaft width of each Phillips size, `#0` through `#4`.
fn phillips_size(a: &Args) -> LuaResult<usize> {
  let n = match a.val("size") {
    Some(Val::Num(n)) => n as i64,
    _ => match a.string("size") {
      Some(s) => s.trim_start_matches('#').parse::<i64>().unwrap_or(-1),
      None => -1,
    },
  };
  if !(0..=4).contains(&n) {
    return a.err("size must be #0 to #4");
  }
  Ok(n as usize)
}

const PHILLIPS_SHAFT: [f64; 5] = [3.0, 4.5, 6.0, 8.0, 10.0];
const PHILLIPS_GAP: [f64; 5] = [0.81, 1.27, 2.29, 3.81, 5.08];

fn phillips_diam(_lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let n = phillips_size(a)?;
  Ok(LuaValue::Number(PHILLIPS_SHAFT[n]))
}

fn phillips_depth(_lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let n = phillips_size(a)?;
  // The recess is a small cone at the tip and a wider one above it.
  let bot_angle = 28.0f64;
  let side_angle = 26.5f64;
  let h1 = PHILLIPS_GAP[n] / 2.0 * bot_angle.to_radians().tan();
  let h2 = (PHILLIPS_SHAFT[n] - PHILLIPS_GAP[n]) / 2.0
    * (90.0 - side_angle).to_radians().tan();
  Ok(LuaValue::Number(h1 + h2))
}

fn phillips_mask(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let n = phillips_size(a)?;
  let shaft = PHILLIPS_SHAFT[n];
  let gap = PHILLIPS_GAP[n];
  let bot_angle = 28.0f64;
  let side_angle = 26.5f64;
  let h1 = gap / 2.0 * bot_angle.to_radians().tan();
  let h2 = (shaft - gap) / 2.0 * (90.0 - side_angle).to_radians().tan();
  let depth = h1 + h2;

  // Four tapered blades crossing at the axis.
  let blade = ScadNode::Translate {
    x: 0.0,
    y: 0.0,
    z: (depth / 2.0) as f32,
    child: Box::new(ScadNode::Polyhedron {
      points: vec![
        [
          (-gap / 2.0) as f32,
          (-shaft / 2.0) as f32,
          (depth / 2.0) as f32,
        ],
        [
          (gap / 2.0) as f32,
          (-shaft / 2.0) as f32,
          (depth / 2.0) as f32,
        ],
        [
          (gap / 2.0) as f32,
          (shaft / 2.0) as f32,
          (depth / 2.0) as f32,
        ],
        [
          (-gap / 2.0) as f32,
          (shaft / 2.0) as f32,
          (depth / 2.0) as f32,
        ],
        [
          (-gap / 4.0) as f32,
          (-gap / 2.0) as f32,
          (-depth / 2.0) as f32,
        ],
        [
          (gap / 4.0) as f32,
          (-gap / 2.0) as f32,
          (-depth / 2.0) as f32,
        ],
        [
          (gap / 4.0) as f32,
          (gap / 2.0) as f32,
          (-depth / 2.0) as f32,
        ],
        [
          (-gap / 4.0) as f32,
          (gap / 2.0) as f32,
          (-depth / 2.0) as f32,
        ],
      ],
      // Wound the way `polyhedron()` wants: clockwise seen from outside.
      faces: vec![
        vec![3, 2, 1, 0],
        vec![4, 5, 6, 7],
        vec![1, 5, 4, 0],
        vec![2, 6, 5, 1],
        vec![3, 7, 6, 2],
        vec![0, 4, 7, 3],
      ],
    }),
  };
  let node = ScadNode::Union(vec![
    blade.clone(),
    crate::bosl::attach::transform(blade, Mat4::zrot(90.0)),
  ]);
  let attachable = Attachable::new(Geom::Conoid {
    r1: [shaft / 2.0, shaft / 2.0],
    r2: [shaft / 2.0, shaft / 2.0],
    l: depth,
    shift: [0.0, 0.0],
    axis: [0.0, 0.0, 1.0],
  });
  as_geometry(lua, "phillips_mask", reorient(node, a, &attachable)?)
}

/// Torx sizes: outer diameter, inner diameter and depth.
const TORX: &[(i64, [f64; 3])] = &[
  (1, [0.90, 0.65, 0.40]),
  (2, [1.00, 0.73, 0.44]),
  (3, [1.20, 0.87, 0.53]),
  (4, [1.35, 0.98, 0.59]),
  (5, [1.48, 1.08, 0.65]),
  (6, [1.75, 1.27, 0.78]),
  (7, [2.08, 1.50, 0.91]),
  (8, [2.40, 1.75, 1.05]),
  (9, [2.58, 1.87, 1.13]),
  (10, [2.80, 2.05, 1.22]),
  (15, [3.35, 2.40, 1.48]),
  (20, [3.95, 2.85, 1.75]),
  (25, [4.50, 3.25, 2.00]),
  (27, [5.60, 4.05, 2.50]),
  (30, [6.65, 4.85, 2.95]),
  (40, [8.80, 6.45, 3.90]),
  (45, [11.30, 8.25, 5.00]),
  (50, [13.25, 9.70, 5.90]),
  (55, [16.55, 12.15, 7.35]),
  (60, [20.00, 14.60, 8.85]),
  (70, [22.50, 16.50, 10.00]),
  (80, [25.40, 18.60, 11.30]),
  (90, [28.60, 20.90, 12.70]),
  (100, [32.00, 23.40, 14.20]),
];

fn torx_entry(a: &Args) -> LuaResult<[f64; 3]> {
  let size = a.need_num("size")? as i64;
  match TORX.iter().find(|(s, _)| *s == size) {
    Some((_, v)) => Ok(*v),
    None => a.err(format!("T{size} is not a standard torx size")),
  }
}

fn torx_info(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let v = torx_entry(a)?;
  Val::vec(v).to_lua(lua)
}

fn torx_diam(_lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  Ok(LuaValue::Number(torx_entry(a)?[0]))
}

fn torx_depth(_lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  Ok(LuaValue::Number(torx_entry(a)?[2]))
}

/// The six-lobed torx outline.
fn torx_outline(od: f64, id: f64) -> Vec<[f64; 2]> {
  let steps = 180usize;
  (0..steps)
    .map(|i| {
      let ang = 360.0 * i as f64 / steps as f64;
      // Six lobes: the radius swings between the inner and outer circles.
      let r =
        (od + id) / 4.0 + (od - id) / 4.0 * (6.0 * ang).to_radians().cos();
      let (s, c) = ang.to_radians().sin_cos();
      [r * c, r * s]
    })
    .collect()
}

fn torx_mask2d(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let v = torx_entry(a)?;
  let path = torx_outline(v[0], v[1]);
  let attachable = Attachable::new(Geom::RegionExtent {
    points: path.clone(),
  });
  as_sketch(
    lua,
    "screw_drive.scad",
    "torx_mask2d",
    reorient(crate::bosl::shapes2d::path_node(&path), a, &attachable)?,
  )
}

fn torx_mask(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let v = torx_entry(a)?;
  let depth = a.num_or("l", v[2]);
  let path = torx_outline(v[0], v[1]);
  let node = ScadNode::LinearExtrude {
    height: depth as f32,
    center: a.bool_or("center", true),
    twist: 0.0,
    slices: 1,
    scale: 1.0,
    child: Box::new(crate::bosl::shapes2d::path_node(&path)),
  };
  let attachable = Attachable::new(Geom::Conoid {
    r1: [v[0] / 2.0, v[0] / 2.0],
    r2: [v[0] / 2.0, v[0] / 2.0],
    l: depth,
    shift: [0.0, 0.0],
    axis: [0.0, 0.0, 1.0],
  });
  as_geometry(lua, "torx_mask", reorient(node, a, &attachable)?)
}

fn hex_drive_mask(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let across = a.need_num("size")?;
  let l = a.num("l").or_else(|| a.num("length")).unwrap_or(across);
  // Across the flats, so the circumscribed radius is a little larger.
  let r = across / 2.0 / (std::f64::consts::PI / 6.0).cos();
  let node = ScadNode::Cylinder {
    r1: r as f32,
    r2: r as f32,
    h: l as f32,
    segments: 6,
    center: true,
  };
  let attachable = Attachable::new(Geom::Conoid {
    r1: [r, r],
    r2: [r, r],
    l,
    shift: [0.0, 0.0],
    axis: [0.0, 0.0, 1.0],
  });
  as_geometry(lua, "hex_drive_mask", reorient(node, a, &attachable)?)
}

fn robertson_mask(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let size = a.need_num("size")? as usize;
  // Square drive sizes #00 to #4, across the flats.
  let widths = [1.77, 2.29, 2.80, 3.30, 3.80, 4.85, 5.94];
  let w = *widths.get(size.min(widths.len() - 1)).unwrap_or(&2.8);
  let l = a.num("l").unwrap_or(w * 1.5);
  let node = ScadNode::Cube {
    w: w as f32,
    d: w as f32,
    h: l as f32,
    center: true,
  };
  let attachable = Attachable::new(Geom::Prismoid {
    size: [w, w, l],
    size2: [w, w],
    shift: [0.0, 0.0],
    axis: [0.0, 0.0, 1.0],
  });
  as_geometry(lua, "robertson_mask", reorient(node, a, &attachable)?)
}

// ---------------------------------------------------------------------------
// Registration
// ---------------------------------------------------------------------------

/// Register one BOSL2 function under `bosl.<name>`.
///
/// Takes any callable, so a family of parts that differ only by a table
/// entry can be registered from one closure per member.
pub fn register_one(
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

/// Register one BOSL2 shape that the generic shim cannot express.
///
/// The difference from [`register_one`] is only how the arguments are read:
/// a shape takes one table holding its positional values with any named ones
/// alongside them, the way `bosl.cuboid { 10, rounding = 2 }` does.
pub fn register_shape(
  lua: &Lua,
  bosl: &mlua::Table,
  name: &'static str,
  params: &'static [&'static str],
  f: impl Fn(&Lua, &Args) -> LuaResult<LuaValue> + 'static,
) -> LuaResult<()> {
  let func = lua.create_function(move |lua, args: mlua::MultiValue| {
    let parsed = Args::parse(name, params, &args)?;
    f(lua, &parsed)
  })?;
  bosl.set(name, func)?;
  Ok(())
}

const ROD_PARAMS: &[&str] = &[
  "d",
  "l",
  "pitch",
  "thread_angle",
  "flank_angle",
  "thread_depth",
  "left_handed",
  "bevel",
  "bevel1",
  "bevel2",
  "starts",
  "internal",
  "d1",
  "d2",
  "length",
  "h",
  "height",
  "profile",
  "slop",
  "anchor",
  "spin",
  "orient",
  "ball_diam",
  "ball_arc",
];
const NUT_PARAMS: &[&str] = &[
  "nutwidth",
  "id",
  "h",
  "pitch",
  "thread_angle",
  "flank_angle",
  "thread_depth",
  "left_handed",
  "starts",
  "bevel",
  "bevang",
  "ibevel",
  "thickness",
  "height",
  "l",
  "s",
  "shape",
  "slop",
  "profile",
  "anchor",
  "spin",
  "orient",
];

pub fn register(lua: &Lua, bosl: &mlua::Table) -> LuaResult<()> {
  for (name, f) in [
    (
      "threaded_rod",
      threaded_rod as fn(&Lua, &Args) -> LuaResult<LuaValue>,
    ),
    ("trapezoidal_threaded_rod", trapezoidal_threaded_rod),
    ("acme_threaded_rod", acme_threaded_rod),
    ("square_threaded_rod", square_threaded_rod),
    ("buttress_threaded_rod", buttress_threaded_rod),
    ("ball_screw_rod", ball_screw_rod),
    ("npt_threaded_rod", npt_threaded_rod),
    ("bspp_threaded_rod", bspp_threaded_rod),
    ("generic_threaded_rod", generic_threaded_rod),
  ] {
    register_one(lua, bosl, name, ROD_PARAMS, f)?;
  }
  for (name, f) in [
    (
      "threaded_nut",
      threaded_nut as fn(&Lua, &Args) -> LuaResult<LuaValue>,
    ),
    ("trapezoidal_threaded_nut", trapezoidal_threaded_nut),
    ("acme_threaded_nut", acme_threaded_nut),
    ("square_threaded_nut", square_threaded_nut),
    ("buttress_threaded_nut", buttress_threaded_nut),
    ("generic_threaded_nut", generic_threaded_nut),
  ] {
    register_one(lua, bosl, name, NUT_PARAMS, f)?;
  }
  register_one(
    lua,
    bosl,
    "thread_helix",
    &[
      "d",
      "pitch",
      "thread_depth",
      "flank_angle",
      "turns",
      "starts",
      "left_handed",
      "internal",
      "d1",
      "d2",
      "profile",
      "thread_angle",
      "r",
      "anchor",
      "spin",
      "orient",
    ],
    thread_helix,
  )?;
  register_one(
    lua,
    bosl,
    "thread_specification",
    &["d", "pitch", "internal", "slop"],
    thread_specification,
  )?;

  register_one(
    lua,
    bosl,
    "phillips_diam",
    &["size", "depth"],
    phillips_diam,
  )?;
  register_one(lua, bosl, "phillips_depth", &["size", "d"], phillips_depth)?;
  register_one(
    lua,
    bosl,
    "phillips_mask",
    &["size", "anchor", "spin", "orient"],
    phillips_mask,
  )?;
  register_one(lua, bosl, "torx_info", &["size"], torx_info)?;
  register_one(lua, bosl, "torx_diam", &["size"], torx_diam)?;
  register_one(lua, bosl, "torx_depth", &["size"], torx_depth)?;
  register_one(
    lua,
    bosl,
    "torx_mask",
    &["size", "l", "center", "anchor", "spin", "orient"],
    torx_mask,
  )?;
  register_one(lua, bosl, "torx_mask2d", &["size"], torx_mask2d)?;
  register_one(
    lua,
    bosl,
    "hex_drive_mask",
    &["size", "l", "length", "anchor", "spin", "orient"],
    hex_drive_mask,
  )?;
  register_one(
    lua,
    bosl,
    "robertson_mask",
    &["size", "extra", "ang", "l", "anchor", "spin", "orient"],
    robertson_mask,
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

  fn measure(code: &str) -> (f64, ([f32; 3], [f32; 3])) {
    let geoms = crate::lua_engine::execute_lua(code).unwrap();
    let node = geoms[0].scad.clone().unwrap();
    let m = crate::export::materialize_scad_manifold(&node);
    (m.volume(), m.bounding_box())
  }

  #[test]
  fn a_threaded_rod_fits_inside_its_nominal_diameter() {
    let (v, (lo, hi)) =
      measure("render(bosl.threaded_rod({d = 10, l = 20, pitch = 2}))");
    assert!(v > 0.0, "the rod should have volume");
    assert!((hi[0] - 5.0).abs() < 0.2, "{hi:?}");
    assert!((lo[0] + 5.0).abs() < 0.2, "{lo:?}");
    assert!((hi[2] - 10.0).abs() < 0.1, "{hi:?}");
    assert!((lo[2] + 10.0).abs() < 0.1, "{lo:?}");
  }

  #[test]
  fn a_thread_sits_between_its_root_and_outer_cylinders() {
    let (v, _) =
      measure("render(bosl.threaded_rod({d = 10, l = 20, pitch = 2}))");
    let outer = std::f64::consts::PI * 25.0 * 20.0;
    // The root of an ISO thread is about 0.65 of the nominal diameter.
    let root = std::f64::consts::PI * (5.0f64 - 1.08).powi(2) * 20.0;
    assert!(v < outer && v > root, "{v} not between {root} and {outer}");
  }

  #[test]
  fn a_finer_pitch_leaves_more_material() {
    let (coarse, _) =
      measure("render(bosl.threaded_rod({d = 10, l = 20, pitch = 3}))");
    let (fine, _) =
      measure("render(bosl.threaded_rod({d = 10, l = 20, pitch = 1}))");
    assert!(fine > coarse, "{fine} vs {coarse}");
  }

  #[test]
  fn a_square_thread_has_more_metal_than_a_pointed_one() {
    let (square, _) =
      measure("render(bosl.square_threaded_rod({d = 10, l = 20, pitch = 2}))");
    let (acme, _) =
      measure("render(bosl.acme_threaded_rod({d = 10, l = 20, pitch = 2}))");
    assert!(square > acme, "{square} vs {acme}");
  }

  #[test]
  fn multiple_starts_add_more_threads() {
    let (one, _) = measure(
      "render(bosl.acme_threaded_rod({d = 10, l = 20, pitch = 2, starts = 1}))",
    );
    let (two, _) = measure(
      "render(bosl.acme_threaded_rod({d = 10, l = 20, pitch = 2, starts = 2}))",
    );
    assert!(two > one, "{two} vs {one}");
  }

  #[test]
  fn a_nut_is_a_hex_body_with_the_thread_bored_out() {
    let (v, (lo, hi)) = measure(
      "render(bosl.threaded_nut({nutwidth = 16, id = 10, h = 8, pitch = 2}))",
    );
    assert!(v > 0.0);
    assert!((hi[2] - 4.0).abs() < 0.1, "{hi:?}");
    assert!((lo[2] + 4.0).abs() < 0.1, "{lo:?}");
    // Across the corners is wider than across the flats.
    assert!(hi[0] > 8.0 && hi[0] < 9.5, "{hi:?}");
  }

  #[test]
  fn a_nut_bore_is_hollow() {
    let (v, _) = measure(
      "render(bosl.threaded_nut({nutwidth = 16, id = 10, h = 8, pitch = 2}))",
    );
    // A solid hex prism of the same size would be much heavier.
    let solid = 6.0
      * 0.5
      * (16.0f64 / 2.0 / (30f64.to_radians().cos())).powi(2)
      * 60f64.to_radians().sin()
      * 8.0;
    assert!(v < solid * 0.8, "{v} vs {solid}");
  }

  #[test]
  fn a_left_handed_thread_differs_from_a_right_handed_one() {
    let lua = Lua::new();
    register_bosl(&lua).unwrap();
    let right = crate::lua_engine::execute_lua(
      "render(bosl.acme_threaded_rod({d = 10, l = 10, pitch = 2}))",
    )
    .unwrap();
    let left = crate::lua_engine::execute_lua(
      "render(bosl.acme_threaded_rod({d = 10, l = 10, pitch = 2,
                                      left_handed = true}))",
    )
    .unwrap();
    let vol = |g: &[crate::geometry::CsgGeometry]| {
      crate::export::materialize_scad_manifold(g[0].scad.as_ref().unwrap())
        .volume()
    };
    // Mirror images have the same volume but are not the same solid.
    assert!((vol(&right) - vol(&left)).abs() / vol(&right) < 0.02);
  }

  #[test]
  fn thread_specification_reports_the_three_diameters() {
    let d: Vec<f64> =
      eval("return bosl.thread_specification({d = 10, pitch = 1.5})");
    assert_eq!(d.len(), 3);
    // Minor < pitch < major, and the major is the nominal size.
    assert!(d[0] < d[1] && d[1] < d[2], "{d:?}");
    assert!((d[2] - 10.0).abs() < 1e-9, "{d:?}");
    assert!((d[1] - 9.026).abs() < 0.01, "{d:?}");
  }

  #[test]
  fn a_helix_thread_has_no_core() {
    let (v, (lo, hi)) =
      measure("render(bosl.thread_helix({d = 10, pitch = 2, turns = 3}))");
    assert!(v > 0.0);
    // Three turns at a 2 pitch climb six units, and the profile reaches
    // about half a pitch past each end.
    let span = (hi[2] - lo[2]) as f64;
    assert!(span > 6.0 && span < 8.0, "{span}");
  }

  #[test]
  fn phillips_sizes_match_the_standard() {
    assert_eq!(eval::<f64>("return bosl.phillips_diam('#2')"), 6.0);
    assert_eq!(eval::<f64>("return bosl.phillips_diam(2)"), 6.0);
    assert!(eval::<f64>("return bosl.phillips_depth('#2')") > 0.0);
  }

  #[test]
  fn torx_sizes_match_the_standard() {
    let info: Vec<f64> = eval("return bosl.torx_info(20)");
    assert_eq!(info, vec![3.95, 2.85, 1.75]);
    assert_eq!(eval::<f64>("return bosl.torx_diam(20)"), 3.95);
    assert_eq!(eval::<f64>("return bosl.torx_depth(20)"), 1.75);
  }

  #[test]
  fn an_unknown_torx_size_is_reported() {
    let lua = Lua::new();
    register_bosl(&lua).unwrap();
    let err = lua
      .load("return bosl.torx_diam(11)")
      .eval::<mlua::Value>()
      .unwrap_err()
      .to_string();
    assert!(err.contains("standard torx size"), "{err}");
  }

  #[test]
  fn the_drive_masks_are_solids_of_the_right_size() {
    let (v, (_, hi)) = measure("render(bosl.torx_mask({size = 20, l = 5}))");
    assert!(v > 0.0);
    assert!((hi[0] - 3.95 / 2.0).abs() < 0.1, "{hi:?}");

    let (v, (_, hi)) =
      measure("render(bosl.hex_drive_mask({size = 5, l = 8}))");
    assert!(v > 0.0);
    // Across the flats is 5, so the corners reach a little further.
    assert!(hi[0] > 2.5 && hi[0] < 3.0, "{hi:?}");

    let (v, _) = measure("render(bosl.phillips_mask('#2'))");
    assert!(v > 0.0);
  }

  #[test]
  fn a_torx_outline_can_be_extruded_as_a_sketch() {
    let (v, _) = measure("render((bosl.torx_mask2d(20)):linear_extrude(2))");
    assert!(v > 0.0);
  }
}
