//! BOSL2's `screws.scad`.
//!
//! A screw is looked up in the ISO metric tables and then built from the
//! thread machinery in [`crate::bosl::threading`]. The tables are the ones
//! BOSL2 carries, so `bosl.screw("M6,20")` has the dimensions a real M6
//! screw has rather than a plausible-looking approximation.

use mlua::{Lua, Result as LuaResult, Table, Value as LuaValue};

use crate::bosl::attach::{Attachable, Geom, reorient_default};
use crate::bosl::threading::{
  build_thread_for, iso_thread_profile, register_shape,
};
use crate::bosl::value::Args;
use crate::bosl::vnf::{Vnf, arc_pts, ccw};
use crate::geometry::CsgGeometry;
use crate::scad_export::ScadNode;

const EPS: f64 = 1e-9;

fn as_geometry(
  lua: &Lua,
  function: &'static str,
  native: ScadNode,
) -> LuaResult<LuaValue> {
  let scad = crate::bosl::bosl_node_with_children(
    "screws.scad",
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

// ---------------------------------------------------------------------------
// The ISO metric tables
// ---------------------------------------------------------------------------

/// Thread pitches by nominal diameter: coarse, fine, extra fine, super fine.
/// A zero means that size has no thread of that fineness.
const ISO_THREAD: &[(f64, [f64; 4])] = &[
  (1.0, [0.25, 0.2, 0.0, 0.0]),
  (1.2, [0.25, 0.2, 0.0, 0.0]),
  (1.4, [0.3, 0.2, 0.0, 0.0]),
  (1.6, [0.35, 0.2, 0.0, 0.0]),
  (1.7, [0.35, 0.0, 0.0, 0.0]),
  (1.8, [0.35, 0.2, 0.0, 0.0]),
  (2.0, [0.4, 0.25, 0.0, 0.0]),
  (2.2, [0.45, 0.25, 0.0, 0.0]),
  (2.3, [0.4, 0.0, 0.0, 0.0]),
  (2.5, [0.45, 0.35, 0.0, 0.0]),
  (2.6, [0.45, 0.0, 0.0, 0.0]),
  (3.0, [0.5, 0.35, 0.0, 0.0]),
  (3.5, [0.6, 0.35, 0.0, 0.0]),
  (4.0, [0.7, 0.5, 0.0, 0.0]),
  (5.0, [0.8, 0.5, 0.0, 0.0]),
  (6.0, [1.0, 0.75, 0.0, 0.0]),
  (7.0, [1.0, 0.75, 0.0, 0.0]),
  (8.0, [1.25, 1.0, 0.75, 0.0]),
  (9.0, [1.25, 1.0, 0.75, 0.0]),
  (10.0, [1.5, 1.25, 1.0, 0.75]),
  (11.0, [1.5, 1.0, 0.75, 0.0]),
  (12.0, [1.75, 1.5, 1.25, 1.0]),
  (14.0, [2.0, 1.5, 1.25, 1.0]),
  (16.0, [2.0, 1.5, 1.0, 0.0]),
  (18.0, [2.5, 2.0, 1.5, 1.0]),
  (20.0, [2.5, 2.0, 1.5, 1.0]),
  (22.0, [2.5, 2.0, 1.5, 1.0]),
  (24.0, [3.0, 2.0, 1.5, 1.0]),
  (27.0, [3.0, 2.0, 1.5, 1.0]),
  (30.0, [3.5, 3.0, 2.0, 1.5]),
  (33.0, [3.5, 3.0, 2.0, 1.5]),
  (36.0, [4.0, 3.0, 2.0, 1.5]),
  (39.0, [4.0, 3.0, 2.0, 1.5]),
  (42.0, [4.5, 4.0, 3.0, 2.0]),
  (45.0, [4.5, 4.0, 3.0, 2.0]),
  (48.0, [5.0, 4.0, 3.0, 2.0]),
  (52.0, [5.0, 4.0, 3.0, 2.0]),
  (56.0, [5.5, 4.0, 3.0, 2.0]),
  (60.0, [5.5, 4.0, 3.0, 2.0]),
  (64.0, [6.0, 4.0, 3.0, 2.0]),
];

/// Socket head diameters (ISO 4762). The height is the screw's own diameter.
const METRIC_SOCKET: &[(f64, f64)] = &[
  (1.4, 2.5),
  (1.6, 3.0),
  (2.0, 3.8),
  (2.5, 4.5),
  (2.6, 5.0),
  (3.0, 5.5),
  (3.5, 6.2),
  (4.0, 7.0),
  (5.0, 8.5),
  (6.0, 10.0),
  (7.0, 12.0),
  (8.0, 13.0),
  (10.0, 16.0),
  (12.0, 18.0),
  (14.0, 21.0),
  (16.0, 24.0),
  (18.0, 27.0),
  (20.0, 30.0),
  (22.0, 33.0),
  (24.0, 36.0),
  (27.0, 40.0),
  (30.0, 45.0),
  (33.0, 50.0),
  (36.0, 54.0),
  (42.0, 63.0),
  (48.0, 72.0),
];

/// Hex head width across the flats, and height.
const METRIC_HEX: &[(f64, [f64; 2])] = &[
  (5.0, [8.0, 3.5]),
  (6.0, [10.0, 4.0]),
  (8.0, [13.0, 5.3]),
  (10.0, [17.0, 6.4]),
  (12.0, [19.0, 7.5]),
  (14.0, [22.0, 8.8]),
  (16.0, [24.0, 10.0]),
  (18.0, [27.0, 11.5]),
  (20.0, [30.0, 12.5]),
  (24.0, [36.0, 15.0]),
  (30.0, [46.0, 18.7]),
];

/// Button head (ISO 7380-1) diameter and height.
const METRIC_BUTTON: &[(f64, [f64; 2])] = &[
  (1.6, [2.9, 0.8]),
  (2.0, [3.5, 1.3]),
  (2.2, [3.8, 0.9]),
  (2.5, [4.6, 1.5]),
  (3.0, [5.7, 1.65]),
  (3.5, [5.7, 1.65]),
  (4.0, [7.6, 2.2]),
  (5.0, [9.5, 2.75]),
  (6.0, [10.5, 3.3]),
  (8.0, [14.0, 4.4]),
  (10.0, [17.5, 5.5]),
  (12.0, [21.0, 6.6]),
  (16.0, [28.0, 8.8]),
];

/// Pan head diameter, then its height slotted and its height for a cross
/// drive — the slotted form is the flatter of the two.
const METRIC_PAN: &[(f64, [f64; 3])] = &[
  (1.6, [3.2, 1.0, 1.3]),
  (2.0, [4.0, 1.3, 1.6]),
  (2.5, [5.0, 1.5, 2.0]),
  (3.0, [5.6, 1.8, 2.4]),
  (3.5, [7.0, 2.1, 3.1]),
  (4.0, [8.0, 2.4, 3.1]),
  (5.0, [9.5, 3.0, 3.8]),
  (6.0, [12.0, 3.6, 4.6]),
  (8.0, [16.0, 4.8, 6.0]),
  (10.0, [20.0, 6.0, 7.5]),
];

/// Cheese head (ISO 1207) diameter and height.
const METRIC_CHEESE: &[(f64, [f64; 2])] = &[
  (1.0, [2.0, 0.7]),
  (1.2, [2.3, 0.8]),
  (1.4, [2.6, 0.9]),
  (1.6, [3.0, 1.0]),
  (2.0, [3.8, 1.3]),
  (2.5, [4.5, 1.6]),
  (3.0, [5.5, 2.0]),
  (3.5, [6.0, 2.4]),
  (4.0, [7.0, 2.6]),
  (5.0, [8.5, 3.3]),
  (6.0, [10.0, 3.9]),
  (8.0, [13.0, 5.0]),
  (10.0, [16.0, 6.0]),
];

/// Countersunk head, small form (ISO 7046): the theoretical sharp diameter
/// and the mean actual one. The screw's form follows the sharp diameter.
const METRIC_FLAT_SMALL: &[(f64, [f64; 2])] = &[
  (1.6, [3.6, 2.85]),
  (2.0, [4.4, 3.65]),
  (2.5, [5.5, 4.55]),
  (3.0, [6.3, 5.35]),
  (3.5, [8.2, 7.07]),
  (4.0, [9.4, 8.18]),
  (5.0, [10.4, 9.15]),
  (6.0, [12.6, 11.09]),
  (8.0, [17.3, 15.44]),
  (10.0, [20.0, 17.79]),
];

/// Countersunk head, large form (ISO 10642), which goes with a hex drive.
const METRIC_FLAT_LARGE: &[(f64, [f64; 2])] = &[
  (3.0, [6.72, 5.54]),
  (4.0, [8.96, 7.53]),
  (5.0, [11.20, 9.43]),
  (6.0, [13.44, 11.34]),
  (8.0, [17.92, 15.24]),
  (10.0, [22.4, 19.22]),
  (12.0, [26.88, 23.12]),
  (14.0, [30.8, 26.52]),
  (16.0, [33.6, 29.01]),
  (20.0, [40.32, 36.05]),
];

/// Nuts (ISO 4032/4035/4033): width across the flats, then the thickness
/// normal, thin and thick. A zero means no nut of that grade is made.
const METRIC_NUT: &[(f64, [f64; 4])] = &[
  (1.6, [3.2, 1.2, 1.0, 0.0]),
  (2.0, [4.0, 1.5, 1.2, 0.0]),
  (2.5, [5.0, 1.875, 1.6, 0.0]),
  (3.0, [5.5, 2.25, 1.8, 0.0]),
  (3.5, [6.0, 2.675, 2.0, 0.0]),
  (4.0, [7.0, 3.0, 2.2, 0.0]),
  (5.0, [8.0, 4.5, 2.7, 5.1]),
  (6.0, [10.0, 5.0, 3.2, 5.7]),
  (8.0, [13.0, 6.675, 0.0, 7.5]),
  (10.0, [16.0, 8.25, 0.0, 9.3]),
  (12.0, [18.0, 10.5, 0.0, 12.0]),
  (14.0, [21.0, 12.5, 0.0, 14.1]),
  (16.0, [24.0, 14.5, 0.0, 16.4]),
  (18.0, [27.0, 15.5, 0.0, 17.6]),
  (20.0, [30.0, 17.5, 0.0, 20.3]),
  (22.0, [34.0, 19.0, 0.0, 21.8]),
  (24.0, [36.0, 21.0, 0.0, 23.9]),
  (27.0, [41.0, 23.0, 0.0, 26.7]),
  (30.0, [46.0, 25.0, 0.0, 28.6]),
  (33.0, [50.0, 28.0, 0.0, 32.5]),
  (36.0, [55.0, 30.0, 0.0, 34.7]),
];

fn look_up<T: Copy>(table: &[(f64, T)], diam: f64) -> Option<T> {
  table
    .iter()
    .find(|(d, _)| (d - diam).abs() < 1e-6)
    .map(|(_, v)| *v)
}

// ---------------------------------------------------------------------------
// Reading a screw name
// ---------------------------------------------------------------------------

/// Everything a screw's name says about it, filled in from the tables.
struct Spec {
  name: String,
  diameter: f64,
  /// Zero for a screw asked for without a thread.
  pitch: f64,
  head: String,
  /// Across the flats for a hex head, otherwise the head's diameter.
  head_size: f64,
  /// The theoretical sharp diameter of a countersunk head.
  head_size_sharp: f64,
  /// How far a head that stands proud of the surface rises above it.
  head_height: f64,
  head_angle: f64,
  length: Option<f64>,
}

impl Spec {
  fn is_headless(&self) -> bool {
    self.head == "none"
  }

  fn is_flat(&self) -> bool {
    self.head.starts_with("flat")
  }

  /// How deep a countersunk head sinks into the part it holds down.
  fn flat_height(&self, shaft_d: f64) -> f64 {
    if !self.is_flat() {
      return 0.0;
    }
    ((self.head_size_sharp - shaft_d)
      / 2.0
      / (self.head_angle / 2.0).to_radians().tan())
    .max(0.0)
  }

  /// The full width of the head, across the corners for a hex one.
  fn head_diam_full(&self) -> f64 {
    if self.head == "hex" {
      2.0 * self.head_size / 3f64.sqrt()
    } else {
      self.head_size
    }
  }
}

/// Which column of the pitch table a thread's fineness picks.
fn thread_column(name: &str) -> Option<usize> {
  match name.to_ascii_lowercase().as_str() {
    "coarse" => Some(0),
    "fine" => Some(1),
    "extra fine" | "extrafine" => Some(2),
    "super fine" | "superfine" => Some(3),
    _ => None,
  }
}

/// Read a name such as `"M4x0.7,10"`: the size, an optional pitch after `x`
/// and an optional length after a comma.
fn parse_name(
  a: &Args,
  spec: &str,
) -> LuaResult<(f64, Option<f64>, Option<f64>)> {
  let (head, length) = match spec.split_once(',') {
    Some((h, l)) => (h.trim(), l.trim().parse::<f64>().ok()),
    None => (spec.trim(), None),
  };
  let (size, pitch) = match head.split_once(['x', 'X']) {
    Some((s, p)) => (s.trim(), p.trim().parse::<f64>().ok()),
    None => (head, None),
  };
  let Some(digits) = size.strip_prefix(['M', 'm']) else {
    return a.err(format!(
      "'{size}' is not a metric screw name; they look like 'M4' or 'M4x0.7'"
    ));
  };
  let Ok(diam) = digits.trim().parse::<f64>() else {
    return a.err(format!("'{size}' has no size after its M"));
  };
  Ok((diam, pitch, length))
}

/// The head dimensions for one size, or an error naming what is missing.
fn head_data(a: &Args, head: &str, diam: f64, drive: &str) -> LuaResult<Spec> {
  let missing = |what: &str| -> String {
    format!("M{diam} has no {what} head in the tables")
  };
  let (size, sharp, height, angle) = match head {
    "none" => (0.0, 0.0, 0.0, 0.0),
    "hex" => match look_up(METRIC_HEX, diam) {
      Some(v) => (v[0], v[0], v[1], 0.0),
      None => return a.err(missing("hex")),
    },
    "socket" | "socket ribbed" => match look_up(METRIC_SOCKET, diam) {
      // A socket head is as tall as the screw is wide.
      Some(d) => (d, d, diam, 0.0),
      None => return a.err(missing("socket")),
    },
    "button" => match look_up(METRIC_BUTTON, diam) {
      Some(v) => (v[0], v[0], v[1], 0.0),
      None => return a.err(missing("button")),
    },
    "cheese" => match look_up(METRIC_CHEESE, diam) {
      Some(v) => (v[0], v[0], v[1], 0.0),
      None => return a.err(missing("cheese")),
    },
    "pan" | "pan round" | "pan flat" => match look_up(METRIC_PAN, diam) {
      Some(v) => {
        let slotted = head == "pan flat" || (head == "pan" && drive == "slot");
        (v[0], v[0], if slotted { v[1] } else { v[2] }, 0.0)
      }
      None => return a.err(missing("pan")),
    },
    _ if head.starts_with("flat") => {
      // The large form goes with a hex drive and the small form with the
      // rest, unless the name says which one outright.
      let large =
        head.contains("large") || (!head.contains("small") && drive == "hex");
      let table = if large {
        METRIC_FLAT_LARGE
      } else {
        METRIC_FLAT_SMALL
      };
      match look_up(table, diam) {
        Some(v) => (v[1], v[0], 0.0, 90.0),
        None => return a.err(missing("countersunk")),
      }
    }
    _ => return a.err(format!("'{head}' is not a head type this knows")),
  };
  Ok(Spec {
    name: String::new(),
    diameter: diam,
    pitch: 0.0,
    head: head.to_string(),
    head_size: size,
    head_size_sharp: sharp,
    head_height: height,
    head_angle: angle,
    length: None,
  })
}

/// Read the whole specification a screw call gives.
fn read_spec(a: &Args, dflt_head: &str) -> LuaResult<Spec> {
  let name = a.string("spec").unwrap_or_else(|| "M4".to_string());
  let (diam, named_pitch, named_length) = parse_name(a, &name)?;

  // `thread` is a fineness name, a pitch, or false for no thread at all.
  let fineness = a.string("thread");
  let pitch =
    if a.bool("thread") == Some(false) || fineness.as_deref() == Some("none") {
      0.0
    } else if let Some(p) = named_pitch.or_else(|| a.num("thread")) {
      p
    } else {
      let named = fineness.as_deref().unwrap_or("coarse");
      let Some(column) = thread_column(named) else {
        return a.err(format!("'{named}' is not a thread fineness"));
      };
      let Some(row) = look_up(ISO_THREAD, diam) else {
        return a.err(format!("M{diam} is not a size in the ISO tables"));
      };
      if row[column] == 0.0 {
        return a.err(format!("M{diam} has no {named} thread"));
      }
      row[column]
    };

  let head = a
    .string("head")
    .unwrap_or_else(|| dflt_head.to_string())
    .to_ascii_lowercase();
  let drive = a.string("drive").unwrap_or_default().to_ascii_lowercase();
  let mut spec = head_data(a, &head, diam, &drive)?;
  spec.name = name;
  spec.pitch = pitch;
  spec.length = a.num("length").or_else(|| a.num("l")).or(named_length);
  Ok(spec)
}

// ---------------------------------------------------------------------------
// The information functions
// ---------------------------------------------------------------------------

fn screw_info(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let s = read_spec(a, "none")?;
  let t: Table = lua.create_table()?;
  t.set("name", s.name.clone())?;
  t.set("system", "ISO")?;
  t.set("diameter", s.diameter)?;
  t.set("pitch", s.pitch)?;
  t.set("head", s.head.clone())?;
  if !s.is_headless() {
    t.set("head_size", s.head_size)?;
    // A countersunk head's height is how deep it sinks, which follows from
    // the shaft it is cut around rather than being tabulated.
    t.set(
      "head_height",
      if s.is_flat() {
        s.flat_height(s.diameter)
      } else {
        s.head_height
      },
    )?;
    if s.is_flat() {
      t.set("head_size_sharp", s.head_size_sharp)?;
      t.set("head_angle", s.head_angle)?;
    }
  }
  if let Some(l) = s.length {
    t.set("length", l)?;
  }
  if let Some(d) = a.string("drive") {
    t.set("drive", d)?;
  }
  Ok(LuaValue::Table(t))
}

/// The nut dimensions the tables give for one screw size.
struct NutSpec {
  diameter: f64,
  pitch: f64,
  width: f64,
  thickness: f64,
  shape: String,
}

fn read_nut(a: &Args) -> LuaResult<NutSpec> {
  let screw = read_spec(a, "none")?;
  let Some(row) = look_up(METRIC_NUT, screw.diameter) else {
    return a.err(format!(
      "M{} is not a nut size in the tables",
      screw.diameter
    ));
  };
  let shape = a
    .string("shape")
    .unwrap_or_else(|| "hex".to_string())
    .to_ascii_lowercase();
  if shape != "hex" && shape != "square" {
    return a.err("a nut's shape is 'hex' or 'square'");
  }
  // The thickness is either a number or one of the standard grades.
  let thickness = match a.num("thickness") {
    Some(t) => t,
    None => {
      let grade = a
        .string("thickness")
        .unwrap_or_else(|| "normal".to_string())
        .to_ascii_lowercase();
      let column = match grade.as_str() {
        "normal" | "din" => 1,
        "thin" | "undersized" => 2,
        "thick" => 3,
        _ => return a.err("thickness is 'thin', 'normal' or 'thick'"),
      };
      if row[column] == 0.0 {
        return a.err(format!(
          "M{} has no {grade} nut in the tables",
          screw.diameter
        ));
      }
      row[column]
    }
  };
  Ok(NutSpec {
    diameter: screw.diameter,
    pitch: screw.pitch,
    width: a
      .num("nutwidth")
      .or_else(|| a.num("width"))
      .unwrap_or(row[0]),
    thickness,
    shape,
  })
}

fn nut_info(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let n = read_nut(a)?;
  let t: Table = lua.create_table()?;
  t.set("name", a.string("spec").unwrap_or_else(|| "M4".to_string()))?;
  t.set("system", "ISO")?;
  t.set("diameter", n.diameter)?;
  t.set("pitch", n.pitch)?;
  t.set("width", n.width)?;
  t.set("thickness", n.thickness)?;
  t.set("shape", n.shape)?;
  Ok(LuaValue::Table(t))
}

// ---------------------------------------------------------------------------
// Building the parts
// ---------------------------------------------------------------------------

fn at(node: ScadNode, z: f64) -> ScadNode {
  if z == 0.0 {
    return node;
  }
  ScadNode::Translate {
    x: 0.0,
    y: 0.0,
    z: z as f32,
    child: Box::new(node),
  }
}

fn cyl(r1: f64, r2: f64, h: f64, segments: u32) -> ScadNode {
  ScadNode::Cylinder {
    r1: r1 as f32,
    r2: r2 as f32,
    h: h as f32,
    segments,
    center: true,
  }
}

/// A prism whose opposite faces are `width` apart, centred on the origin.
fn across_flats(width: f64, sides: u32, h: f64) -> ScadNode {
  let r = width / 2.0 / (180.0 / sides as f64).to_radians().cos();
  cyl(r, r, h, sides)
}

/// A dome of the given base diameter and height, as a revolved arc.
fn dome(d: f64, h: f64, segments: u32) -> ScadNode {
  let base = d / 2.0;
  if h <= EPS || base <= EPS {
    return cyl(base, base, h.max(EPS), segments);
  }
  // The one circle that passes through both the rim and the crown.
  let big = (h * h + base * base) / (2.0 * h);
  let cp = [0.0, h - big];
  let start = (0.0 - cp[1]).atan2(base).to_degrees();
  let n = (segments / 4).max(3);
  let arc = arc_pts(n + 1, big, cp, start, 90.0 - start, true);
  let mut profile = vec![[0.0, 0.0]];
  profile.extend(arc.iter().map(|p| [p[0].max(0.0), p[1]]));
  Vnf::rotate_sweep(&ccw(profile), 360.0, segments, true).to_node()
}

/// The head on its own, sitting on the XY plane with the shaft below it.
///
/// A countersunk head is the exception: it reaches *down* from the plane,
/// because that is where the material it sinks into is.
fn head_node(a: &Args, s: &Spec, shaft_d: f64) -> ScadNode {
  let segments = a.segments(s.head_size.max(shaft_d) / 2.0);
  match s.head.as_str() {
    "none" => ScadNode::Union(vec![]),
    "hex" => at(
      across_flats(s.head_size, 6, s.head_height),
      s.head_height / 2.0,
    ),
    "button" | "pan" | "pan round" => {
      dome(s.head_size, s.head_height, segments)
    }
    _ if s.is_flat() => {
      let h = s.flat_height(shaft_d);
      at(
        cyl(shaft_d / 2.0, s.head_size_sharp / 2.0, h, segments),
        -h / 2.0,
      )
    }
    // Socket, cheese and slotted pan heads are all plain cylinders.
    _ => at(
      cyl(
        s.head_size / 2.0,
        s.head_size / 2.0,
        s.head_height,
        segments,
      ),
      s.head_height / 2.0,
    ),
  }
}

/// The shaft, hanging below the XY plane: `thread_len` of it threaded at the
/// far end and the rest left as a plain shank.
fn shaft_node(
  a: &Args,
  d: f64,
  pitch: f64,
  length: f64,
  thread_len: f64,
) -> ScadNode {
  let segments = a.segments(d / 2.0);
  let thread_len = thread_len.clamp(0.0, length);
  let plain = length - thread_len;
  let mut parts = Vec::new();
  if plain > EPS {
    parts.push(at(cyl(d / 2.0, d / 2.0, plain, segments), -plain / 2.0));
  }
  if thread_len > EPS {
    let threaded = if pitch > 0.0 {
      build_thread_for(
        d / 2.0,
        thread_len,
        pitch,
        1,
        false,
        &iso_thread_profile(),
        segments,
      )
    } else {
      cyl(d / 2.0, d / 2.0, thread_len, segments)
    };
    parts.push(at(threaded, -plain - thread_len / 2.0));
  }
  ScadNode::Union(parts)
}

/// Centre a part that was built spanning `bottom` to `top`, and place it.
///
/// BOSL2's default anchor type for a screw is the whole screw, so `CENTER`
/// puts the middle of head-plus-shaft at the origin.
fn centred(
  a: &Args,
  node: ScadNode,
  bottom: f64,
  top: f64,
  diam: f64,
) -> LuaResult<ScadNode> {
  let attachable = Attachable::new(Geom::Conoid {
    r1: [diam / 2.0; 2],
    r2: [diam / 2.0; 2],
    l: top - bottom,
    shift: [0.0, 0.0],
    axis: [0.0, 0.0, 1.0],
  });
  reorient_default(
    at(node, -(top + bottom) / 2.0),
    a,
    &attachable,
    [0.0, 0.0, 0.0],
    [0.0, 0.0, 0.0],
  )
}

fn screw(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let s = read_spec(a, "socket")?;
  let Some(length) = s.length else {
    return a.err("a screw needs a length, as 'M6,20' or length = 20");
  };
  // A countersunk head's depth counts towards the screw's length; any other
  // head sits on top of it.
  let flat = s.flat_height(s.diameter);
  let shaft_len = length - flat;
  let thread_len = a.num("thread_len").unwrap_or(shaft_len);
  // The shaft starts where the countersink stops, so the length a
  // countersunk screw is sold by is the whole of it.
  let node = ScadNode::Union(vec![
    at(
      shaft_node(a, s.diameter, s.pitch, shaft_len, thread_len),
      -flat,
    ),
    head_node(a, &s, s.diameter),
  ]);
  let widest = s.diameter.max(s.head_diam_full());
  as_geometry(
    lua,
    "screw",
    centred(a, node, -length, s.head_height, widest)?,
  )
}

fn screw_head(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let s = read_spec(a, "socket")?;
  let flat = s.flat_height(s.diameter);
  let node = head_node(a, &s, s.diameter);
  let (bottom, top) = if s.is_flat() {
    (-flat, 0.0)
  } else {
    (0.0, s.head_height)
  };
  as_geometry(
    lua,
    "screw_head",
    centred(a, node, bottom, top, s.head_diam_full())?,
  )
}

fn screw_hole(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let mut s = read_spec(a, "none")?;
  let Some(length) = s.length else {
    return a.err("a screw hole needs a length, as 'M6,20' or length = 20");
  };
  // A hole is cut oversize by the printer's slop — four times over, since
  // BOSL2 counts it on both walls of both the hole and its mate — plus
  // anything the call asks for on top.
  let slop = a.num("slop").unwrap_or_else(crate::bosl::get_slop);
  let over = a
    .num("hole_oversize")
    .or_else(|| a.num("oversize"))
    .unwrap_or(0.0);
  let head_over = a
    .num("head_oversize")
    .or_else(|| a.num("oversize"))
    .unwrap_or(0.0);
  let d = s.diameter + over + 4.0 * slop;
  s.head_size += head_over + 4.0 * slop;
  s.head_size_sharp += head_over + 4.0 * slop;

  // A hole is only threaded when asked; a clearance hole is the common case.
  let threaded = a.bool("thread") == Some(true)
    || a.num("thread").is_some()
    || a
      .string("thread")
      .is_some_and(|t| thread_column(&t).is_some());

  let flat = s.flat_height(d);
  let shaft_len = length - flat;
  let node = ScadNode::Union(vec![
    at(
      shaft_node(
        a,
        d,
        if threaded { s.pitch } else { 0.0 },
        shaft_len,
        if threaded { shaft_len } else { 0.0 },
      ),
      -flat,
    ),
    head_node(a, &s, d),
  ]);
  let widest = d.max(s.head_diam_full());
  as_geometry(
    lua,
    "screw_hole",
    centred(a, node, -length, s.head_height, widest)?,
  )
}

fn shoulder_screw(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let s = read_spec(a, "socket")?;
  let Some(d) = a.num("d") else {
    return a.err("a shoulder screw needs its shoulder diameter, d");
  };
  let Some(shoulder_len) = a.num("length").or_else(|| a.num("l")) else {
    return a.err("a shoulder screw needs its shoulder length");
  };
  // The threaded end is as long as the screw is thick unless told otherwise.
  let thread_len = a.num("thread_len").unwrap_or(s.diameter);
  let segments = a.segments(d / 2.0);
  let flat = s.flat_height(d);

  let node = ScadNode::Union(vec![
    // The plain ground shoulder is what the screw is for: it runs in a
    // bearing rather than clamping, so the thread only starts below it.
    at(
      cyl(d / 2.0, d / 2.0, shoulder_len, segments),
      -flat - shoulder_len / 2.0,
    ),
    at(
      shaft_node(a, s.diameter, s.pitch, thread_len, thread_len),
      -flat - shoulder_len,
    ),
    head_node(a, &s, d),
  ]);
  let widest = d.max(s.head_diam_full());
  as_geometry(
    lua,
    "shoulder_screw",
    centred(
      a,
      node,
      -flat - shoulder_len - thread_len,
      s.head_height,
      widest,
    )?,
  )
}

fn nut(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let n = read_nut(a)?;
  let slop = a.num("slop").unwrap_or_else(crate::bosl::get_slop);
  let segments = a.segments(n.diameter / 2.0);
  let body = if n.shape == "square" {
    ScadNode::Cube {
      w: n.width as f32,
      d: n.width as f32,
      h: n.thickness as f32,
      center: true,
    }
  } else {
    across_flats(n.width, 6, n.thickness)
  };
  // The bore runs past both faces, so the difference leaves no skin behind.
  let bore_d = n.diameter + 4.0 * slop;
  let bore = if n.pitch > 0.0 {
    build_thread_for(
      bore_d / 2.0,
      n.thickness + n.pitch,
      n.pitch,
      1,
      false,
      &iso_thread_profile(),
      segments,
    )
  } else {
    cyl(bore_d / 2.0, bore_d / 2.0, n.thickness + 1.0, segments)
  };
  as_geometry(
    lua,
    "nut",
    centred(
      a,
      ScadNode::Difference(vec![body, bore]),
      -n.thickness / 2.0,
      n.thickness / 2.0,
      n.width,
    )?,
  )
}

fn nut_trap_inline(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let n = read_nut(a)?;
  let length = a
    .num("length")
    .or_else(|| a.num("l"))
    .or_else(|| a.num("h"))
    .or_else(|| a.num("height"));
  let Some(length) = length else {
    return a.err("a nut trap needs a length");
  };
  let width =
    n.width + 2.0 * a.num("slop").unwrap_or_else(crate::bosl::get_slop);
  let node = if n.shape == "square" {
    ScadNode::Cube {
      w: width as f32,
      d: width as f32,
      h: length as f32,
      center: true,
    }
  } else {
    across_flats(width, 6, length)
  };
  as_geometry(
    lua,
    "nut_trap_inline",
    centred(a, node, -length / 2.0, length / 2.0, width)?,
  )
}

fn nut_trap_side(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let n = read_nut(a)?;
  let trap_width = a.num("trap_width").or_else(|| a.num("width"));
  let Some(trap_width) = trap_width else {
    return a.err("a side nut trap needs its trap_width");
  };
  let slop = a.num("slop").unwrap_or_else(crate::bosl::get_slop);
  let width = n.width + 2.0 * slop;
  let thickness = n.thickness + 2.0 * slop;
  if trap_width < width / 2.0 {
    return a.err(format!("trap_width is narrower than the nut, {width}"));
  }
  let half = if n.shape == "square" {
    width / 2.0
  } else {
    width / 3f64.sqrt()
  };

  // The nut sits at the origin and the slot it slid in along runs out to +X.
  let slot = |w: f64, from: f64| ScadNode::Translate {
    x: (from + w / 2.0) as f32,
    y: 0.0,
    z: 0.0,
    child: Box::new(ScadNode::Cube {
      w: w as f32,
      d: width as f32,
      h: thickness as f32,
      center: true,
    }),
  };
  let mut node = if n.shape == "square" {
    slot(trap_width + half, -width / 2.0)
  } else {
    ScadNode::Union(vec![
      slot(trap_width, 0.0),
      across_flats(width, 6, thickness),
    ])
  };
  // A poke hole lets a rod push the nut back out again.
  if let Some(poke_len) = a.num("poke_len").filter(|v| *v > 0.0) {
    let poke_d = a.num("poke_diam").unwrap_or(thickness);
    node = ScadNode::Union(vec![
      node,
      ScadNode::Translate {
        x: (-poke_len / 2.0) as f32,
        y: 0.0,
        z: 0.0,
        child: Box::new(ScadNode::Rotate {
          x: 0.0,
          y: 90.0,
          z: 0.0,
          child: Box::new(cyl(
            poke_d / 2.0,
            poke_d / 2.0,
            poke_len,
            a.segments(poke_d / 2.0),
          )),
        }),
      },
    ]);
  }

  let attachable = Attachable::new(Geom::Prismoid {
    size: [trap_width + half, width, thickness],
    size2: [trap_width + half, width],
    shift: [0.0, 0.0],
    axis: [0.0, 0.0, 1.0],
  })
  .with_offset([trap_width / 2.0 - half / 2.0, 0.0, 0.0]);
  as_geometry(
    lua,
    "nut_trap_side",
    reorient_default(node, a, &attachable, [0.0, 0.0, -1.0], [0.0, 0.0, -1.0])?,
  )
}

// ---------------------------------------------------------------------------
// Registration
// ---------------------------------------------------------------------------

const SCREW_PARAMS: &[&str] = &[
  "spec",
  "head",
  "drive",
  "thread",
  "drive_size",
  "length",
  "l",
  "thread_len",
  "shank",
  "details",
  "tolerance",
  "blunt_start",
  "undersize",
  "shaft_undersize",
  "head_undersize",
  "oversize",
  "hole_oversize",
  "head_oversize",
  "counterbore",
  "teardrop",
  "bevel",
  "bevel1",
  "bevel2",
  "atype",
  "anchor",
  "spin",
  "orient",
  "slop",
  "fn",
];

const NUT_PARAMS: &[&str] = &[
  "spec",
  "shape",
  "thickness",
  "nutwidth",
  "thread",
  "tolerance",
  "hole_oversize",
  "bevel",
  "bevang",
  "ibevel",
  "width",
  "length",
  "l",
  "h",
  "height",
  "trap_width",
  "poke_len",
  "poke_diam",
  "atype",
  "anchor",
  "spin",
  "orient",
  "slop",
  "fn",
];

pub fn register(lua: &Lua, bosl: &Table) -> LuaResult<()> {
  register_shape(lua, bosl, "screw", SCREW_PARAMS, screw)?;
  register_shape(lua, bosl, "screw_hole", SCREW_PARAMS, screw_hole)?;
  register_shape(lua, bosl, "screw_head", SCREW_PARAMS, screw_head)?;
  register_shape(lua, bosl, "screw_info", SCREW_PARAMS, screw_info)?;
  register_shape(
    lua,
    bosl,
    "shoulder_screw",
    &[
      "spec",
      "d",
      "length",
      "l",
      "head",
      "thread_len",
      "tolerance",
      "head_size",
      "drive",
      "drive_size",
      "thread",
      "shank",
      "details",
      "atype",
      "anchor",
      "spin",
      "orient",
      "slop",
      "fn",
    ],
    shoulder_screw,
  )?;
  register_shape(lua, bosl, "nut", NUT_PARAMS, nut)?;
  register_shape(lua, bosl, "nut_info", NUT_PARAMS, nut_info)?;
  register_shape(lua, bosl, "nut_trap_side", NUT_PARAMS, nut_trap_side)?;
  register_shape(lua, bosl, "nut_trap_inline", NUT_PARAMS, nut_trap_inline)?;
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

  fn fails(code: &str) -> String {
    let lua = Lua::new();
    register_bosl(&lua).unwrap();
    lua
      .load(code)
      .eval::<mlua::Value>()
      .expect_err("expected an error")
      .to_string()
  }

  fn measure(code: &str) -> (f64, ([f32; 3], [f32; 3])) {
    let geoms = crate::lua_engine::execute_lua(code).unwrap();
    let node = geoms[0].scad.clone().unwrap();
    let m = crate::export::materialize_scad_manifold(&node);
    (m.volume(), m.bounding_box())
  }

  #[test]
  fn a_screw_name_is_read_apart() {
    assert_eq!(eval::<f64>("return bosl.screw_info('M4').diameter"), 4.0);
    assert_eq!(eval::<f64>("return bosl.screw_info('M4').pitch"), 0.7);
    // An explicit pitch and length override what the tables say.
    assert_eq!(
      eval::<f64>("return bosl.screw_info('M4x0.5,20').pitch"),
      0.5
    );
    assert_eq!(
      eval::<f64>("return bosl.screw_info('M4x0.5,20').length"),
      20.0
    );
    // As does a named fineness.
    assert_eq!(
      eval::<f64>("return bosl.screw_info({'M10', thread = 'fine'}).pitch"),
      1.25
    );
  }

  #[test]
  fn an_unknown_screw_size_is_reported() {
    assert!(fails("return bosl.screw_info('M99')").contains("ISO tables"));
    assert!(fails("return bosl.screw_info('4mm')").contains("metric screw"));
    assert!(
      fails("return bosl.screw_info({'M1.6', thread = 'super fine'})")
        .contains("super fine")
    );
  }

  #[test]
  fn the_head_tables_are_the_iso_ones() {
    let size: f64 =
      eval("return bosl.screw_info({'M6', head = 'socket'}).head_size");
    assert_eq!(size, 10.0);
    // A socket head is as tall as the screw is wide.
    let h: f64 =
      eval("return bosl.screw_info({'M6', head = 'socket'}).head_height");
    assert_eq!(h, 6.0);
    let hex: f64 =
      eval("return bosl.screw_info({'M6', head = 'hex'}).head_size");
    assert_eq!(hex, 10.0);
    let button: f64 =
      eval("return bosl.screw_info({'M6', head = 'button'}).head_size");
    assert_eq!(button, 10.5);
  }

  #[test]
  fn a_countersunk_head_reports_how_deep_it_sinks() {
    // The 90 degree cone from the sharp diameter down to the shaft.
    let h: f64 =
      eval("return bosl.screw_info({'M6', head = 'flat'}).head_height");
    assert!((h - (12.6 - 6.0) / 2.0).abs() < 1e-9, "{h}");
  }

  #[test]
  fn nut_info_reports_the_matching_nut() {
    assert_eq!(eval::<f64>("return bosl.nut_info('M6').width"), 10.0);
    assert_eq!(eval::<f64>("return bosl.nut_info('M6').thickness"), 5.0);
    assert_eq!(
      eval::<f64>("return bosl.nut_info({'M6', thickness = 'thin'}).thickness"),
      3.2
    );
    assert!(
      fails("return bosl.nut_info({'M8', thickness = 'thin'})")
        .contains("no thin nut")
    );
  }

  #[test]
  fn a_screw_spans_its_length_plus_its_head() {
    let (v, (lo, hi)) = measure("render(bosl.screw('M6,20'))");
    assert!(v > 0.0);
    // A socket head is 6 mm tall on an M6, so the pair is 26 mm centred.
    assert!(((hi[2] - lo[2]) as f64 - 26.0).abs() < 0.2, "{lo:?} {hi:?}");
    assert!((lo[2] + hi[2]).abs() < 0.2, "not centred: {lo:?} {hi:?}");
    // The head is the widest part, at 10 mm across.
    assert!((hi[0] as f64 - 5.0).abs() < 0.2, "{hi:?}");
  }

  #[test]
  fn a_countersunk_screw_is_as_long_as_it_says() {
    let (_, (lo, hi)) = measure("render(bosl.screw({'M6,20', head = 'flat'}))");
    // The head sinks into the length rather than adding to it.
    assert!(((hi[2] - lo[2]) as f64 - 20.0).abs() < 0.3, "{lo:?} {hi:?}");
  }

  #[test]
  fn a_headless_screw_is_only_its_shaft() {
    let (_, (lo, hi)) = measure("render(bosl.screw({'M6,20', head = 'none'}))");
    assert!(((hi[2] - lo[2]) as f64 - 20.0).abs() < 0.2, "{lo:?} {hi:?}");
    assert!((hi[0] as f64 - 3.0).abs() < 0.2, "{hi:?}");
  }

  #[test]
  fn a_nut_is_hexagonal_and_bored_through() {
    let (v, (_, hi)) = measure("render(bosl.nut('M6'))");
    let across_corners = 10.0 / 30f64.to_radians().cos();
    assert!((hi[0] as f64 - across_corners / 2.0).abs() < 0.05, "{hi:?}");
    assert!((hi[2] as f64 - 5.0 / 2.0).abs() < 0.05, "{hi:?}");
    // A hexagon of that width, 5 mm thick, less the thread's bore.
    let solid = 3f64.sqrt() / 2.0 * 10.0f64.powi(2) * 5.0;
    let bore = std::f64::consts::PI * 3.0f64.powi(2) * 5.0;
    assert!(v < solid - bore * 0.7, "{v} against {solid} less {bore}");
    assert!(v > solid - bore, "{v} against {solid} less {bore}");
  }

  #[test]
  fn a_square_nut_is_square() {
    let (_, (_, hi)) = measure("render(bosl.nut({'M6', shape = 'square'}))");
    assert!((hi[0] as f64 - 5.0).abs() < 0.05, "{hi:?}");
    assert!((hi[1] as f64 - 5.0).abs() < 0.05, "{hi:?}");
  }

  #[test]
  fn a_clearance_hole_is_wider_than_the_screw() {
    let (screw, _) = measure("render(bosl.screw({'M6,20', head = 'none'}))");
    let (hole, _) =
      measure("render(bosl.screw_hole({'M6,20', hole_oversize = 0.5}))");
    // The hole is a plain cylinder, so it beats the threaded shaft outright.
    assert!(hole > screw, "{hole} against {screw}");
  }

  #[test]
  fn the_screw_parts_all_build_something() {
    for call in [
      "bosl.screw('M4,20')",
      "bosl.screw({'M4,20', head = 'button'})",
      "bosl.screw({'M8,20', head = 'hex'})",
      "bosl.screw({'M4,20', head = 'pan'})",
      "bosl.screw({'M4,20', head = 'cheese'})",
      "bosl.screw_hole('M4,20')",
      "bosl.screw_head('M4')",
      "bosl.shoulder_screw({'M6', d = 8, length = 20})",
      "bosl.nut_trap_side({'M4', trap_width = 12})",
      "bosl.nut_trap_inline({'M4', length = 6})",
    ] {
      let (v, _) = measure(&format!("render({call})"));
      assert!(v > 0.0, "{call} produced nothing");
    }
  }

  #[test]
  fn an_inline_nut_trap_is_the_nut_it_holds() {
    let (v, (_, hi)) = measure("render(bosl.nut_trap_inline({'M6', l = 4}))");
    let across_corners = 10.0 / 30f64.to_radians().cos();
    assert!((hi[0] as f64 - across_corners / 2.0).abs() < 0.05, "{hi:?}");
    let ideal = 3f64.sqrt() / 2.0 * 10.0f64.powi(2) * 4.0;
    assert!((v - ideal).abs() / ideal < 0.01, "{v} against {ideal}");
  }

  #[test]
  fn a_side_nut_trap_has_the_slot_it_slides_in_along() {
    let (_, (lo, hi)) =
      measure("render(bosl.nut_trap_side({'M6', trap_width = 15}))");
    // The slot runs out to +X from the nut at the origin.
    assert!((hi[0] as f64 - 15.0).abs() < 0.1, "{hi:?}");
    assert!(lo[0] < -4.0, "{lo:?}");
    // It is anchored on its bottom face.
    assert!(lo[2].abs() < 0.05, "{lo:?}");
  }

  #[test]
  fn a_shoulder_screw_has_a_plain_shoulder_below_its_head() {
    let (_, (lo, hi)) =
      measure("render(bosl.shoulder_screw({'M6', d = 8, length = 20}))");
    // The shoulder is wider than the thread, so it sets the width.
    assert!((hi[0] as f64 - 5.0).abs() < 0.2, "{hi:?}");
    // Head, shoulder and thread together.
    assert!(((hi[2] - lo[2]) as f64 - (6.0 + 20.0 + 6.0)).abs() < 0.3);
  }
}
