//! BOSL2 (Belfry OpenSCAD Library v2) support for LuaCAD.
//!
//! All functions are registered under the `bosl` Lua namespace table so they
//! can be called as `bosl.cuboid(...)`, `bosl.cyl(...)`, etc.
//!
//! Shapes are built from LuaCAD's own primitives, so `bosl.*` renders,
//! previews and exports to a mesh without OpenSCAD or the BOSL2 library
//! installed. Exporting to `.scad` still writes the BOSL2 call itself,
//! together with the `include <BOSL2/std.scad>` directive and any extra
//! module includes (threading, gears, screws, …), which keeps the exported
//! file as short and readable as the script that produced it.

pub mod args;
pub mod attach;
pub mod attachments;
pub mod beziers;
pub mod coords;
pub mod distributors;
pub mod edges;
pub mod extras;
pub mod gears;
pub mod geom;
pub mod heightfield;
pub mod isosurface;
pub mod linalg;
pub mod lists;
pub mod masks;
pub mod math;
pub mod metric;
pub mod nurbs;
pub mod offset2d;
pub mod parts;
pub mod parts_extra;
pub mod paths;
pub mod regions;
pub mod rounding;
pub mod screws;
pub mod shapes2d;
pub mod shapes3d;
pub mod sweeps;
pub mod textures;
pub mod threading;
pub mod transforms;
pub mod turtle;
pub mod value;
pub mod vecmath;
pub mod vectors;
pub mod vnf;
pub mod vnf_lua;

#[cfg(feature = "csgrs")]
use csgrs::mesh::Mesh as CsgMesh;
#[cfg(feature = "csgrs")]
use csgrs::traits::CSG;
use mlua::{Lua, Result as LuaResult, Value as LuaValue};

use crate::geometry::{CsgGeometry, CsgSketch};
use crate::scad_export::{BoslPreviewParams, CylAxis, ScadNode};

// ---------------------------------------------------------------------------
// Helpers: Lua value → OpenSCAD argument string
// ---------------------------------------------------------------------------

/// Format a Lua value as an OpenSCAD argument string fragment.
pub(crate) fn lua_val_to_scad(v: &LuaValue) -> String {
  match v {
    LuaValue::Number(n) => format_f64(*n),
    LuaValue::Integer(n) => n.to_string(),
    LuaValue::Boolean(b) => b.to_string(),
    LuaValue::String(s) => {
      let s = s.to_str().map(|s| s.to_string()).unwrap_or_default();
      format!("\"{}\"", s)
    }
    LuaValue::Table(t) => lua_table_to_scad_array(t),
    // A 2D shape handed to something that wants an outline — a sweep, a
    // prism to join — writes out as the outline itself. Left as `undef` the
    // exported call would be missing the very thing it works on.
    LuaValue::UserData(ud) => match ud.borrow::<CsgSketch>() {
      Ok(sketch) => match sweeps::outline_of(sketch.scad.as_ref()) {
        Some(path) => format!(
          "[{}]",
          path
            .iter()
            .map(|p| format!("[{}, {}]", format_f64(p[0]), format_f64(p[1])))
            .collect::<Vec<_>>()
            .join(", ")
        ),
        None => "undef".to_string(),
      },
      Err(_) => "undef".to_string(),
    },
    LuaValue::Nil => "undef".to_string(),
    _ => "undef".to_string(),
  }
}

/// Format an f64 trimming trailing zeros.
fn format_f64(v: f64) -> String {
  let s = format!("{:.6}", v);
  let s = s.trim_end_matches('0');
  let s = s.trim_end_matches('.');
  s.to_string()
}

/// Convert a Lua table to an OpenSCAD array like `[1, 2, 3]`.
///
/// A table with only string keys is one of BOSL2's structs — the end
/// treatment an `os_*` constructor hands back, say — which OpenSCAD spells as
/// a flat list of alternating keys and values, `["type", "circle", "r", 3]`.
/// Reading only the array part would export it as `[]`, which BOSL2 reads as
/// "no treatment at all" and silently builds the wrong shape.
fn lua_table_to_scad_array(t: &mlua::Table) -> String {
  let len = t.len().unwrap_or(0);
  if len > 0 {
    let mut parts = Vec::new();
    for i in 1..=len {
      if let Ok(v) = t.get::<LuaValue>(i) {
        parts.push(lua_val_to_scad(&v));
      }
    }
    return format!("[{}]", parts.join(", "));
  }

  let mut fields: Vec<(String, String)> = Vec::new();
  if let Ok(pairs) = t
    .pairs::<LuaValue, LuaValue>()
    .collect::<Result<Vec<_>, _>>()
  {
    for (k, v) in pairs {
      if let LuaValue::String(key) = k {
        let key = key.to_str().map(|s| s.to_string()).unwrap_or_default();
        fields.push((key, lua_val_to_scad(&v)));
      }
    }
  }
  if fields.is_empty() {
    return "[]".to_string();
  }
  // Sorted so the same struct always exports the same way, with the two
  // fields that say what the struct is for leading.
  fields.sort_by(|a, b| {
    let rank = |k: &str| match k {
      "for" => 0,
      "type" => 1,
      _ => 2,
    };
    rank(&a.0).cmp(&rank(&b.0)).then_with(|| a.0.cmp(&b.0))
  });
  let parts: Vec<String> = fields
    .iter()
    .flat_map(|(k, v)| [format!("\"{k}\""), v.clone()])
    .collect();
  format!("[{}]", parts.join(", "))
}

/// Convert a Lua arguments table `{ positional..., named_key = val, ... }`
/// into an OpenSCAD argument string like `[10, 20], fillet = 2, center = true`.
///
/// Positional (integer-keyed) values are emitted first, then named keys
/// in alphabetical order.
pub(crate) fn lua_table_to_scad_args(t: &mlua::Table) -> String {
  let mut positional = Vec::new();
  let mut named = Vec::new();

  // Collect positional args
  let len = t.len().unwrap_or(0);
  for i in 1..=len {
    if let Ok(v) = t.get::<LuaValue>(i) {
      positional.push(lua_val_to_scad(&v));
    }
  }

  // Collect named args
  if let Ok(pairs) = t
    .pairs::<LuaValue, LuaValue>()
    .collect::<Result<Vec<_>, _>>()
  {
    for (k, v) in pairs {
      if let LuaValue::String(key) = k {
        let key_str = key.to_str().map(|s| s.to_string()).unwrap_or_default();
        named.push((key_str, lua_val_to_scad(&v)));
      }
    }
  }
  named.sort_by(|a, b| a.0.cmp(&b.0));

  let mut parts = positional;
  for (k, v) in named {
    parts.push(format!("{} = {}", k, v));
  }
  parts.join(", ")
}

// ---------------------------------------------------------------------------
// Preview parameter extraction
// ---------------------------------------------------------------------------

/// Extract a float from a Lua table by string key.
fn table_get_f64(t: &mlua::Table, key: &str) -> Option<f64> {
  match t.get::<LuaValue>(key).ok()? {
    LuaValue::Number(n) => Some(n),
    LuaValue::Integer(n) => Some(n as f64),
    _ => None,
  }
}

/// Extract a float from a Lua table by integer key.
fn table_get_f64_idx(t: &mlua::Table, idx: i64) -> Option<f64> {
  match t.get::<LuaValue>(idx).ok()? {
    LuaValue::Number(n) => Some(n),
    LuaValue::Integer(n) => Some(n as f64),
    _ => None,
  }
}

/// Extract a bool from a Lua table by string key.
fn table_get_bool(t: &mlua::Table, key: &str) -> Option<bool> {
  match t.get::<LuaValue>(key).ok()? {
    LuaValue::Boolean(b) => Some(b),
    _ => None,
  }
}

/// Extract the size as (w, d, h) from a cuboid argument table.
/// Handles two calling conventions:
///   `bosl.cuboid { {40,40,40} }`  — first positional is a sub-table
///   `bosl.cuboid({40,40,40})`     — positional args are bare numbers
fn extract_cuboid_size(t: &mlua::Table) -> Option<(f64, f64, f64)> {
  // Try sub-table first: t[1] is a table
  if let Ok(inner) = t.get::<mlua::Table>(1) {
    let x = table_get_f64_idx(&inner, 1)?;
    let y = table_get_f64_idx(&inner, 2)?;
    let z = table_get_f64_idx(&inner, 3)?;
    return Some((x, y, z));
  }
  // Fallback: bare numbers at positions 1, 2, 3
  let x = table_get_f64_idx(t, 1)?;
  let y = table_get_f64_idx(t, 2).unwrap_or(x);
  let z = table_get_f64_idx(t, 3).unwrap_or(y);
  Some((x, y, z))
}

fn extract_cuboid_preview(t: &mlua::Table) -> BoslPreviewParams {
  let (w, d, h) = extract_cuboid_size(t).unwrap_or((1.0, 1.0, 1.0));
  // BOSL2 cuboid defaults center=true (via anchor=CENTER)
  let center = table_get_bool(t, "center").unwrap_or(true);
  let rounding = table_get_f64(t, "rounding").unwrap_or(0.0) as f32;
  BoslPreviewParams::Cuboid {
    w: w as f32,
    d: d as f32,
    h: h as f32,
    rounding,
    center,
  }
}

fn extract_cyl_preview(t: &mlua::Table, axis: CylAxis) -> BoslPreviewParams {
  let h = table_get_f64(t, "h")
    .or_else(|| table_get_f64(t, "l"))
    .unwrap_or(1.0);

  let r_uniform =
    table_get_f64(t, "r").or_else(|| table_get_f64(t, "d").map(|d| d / 2.0));

  let r1 = table_get_f64(t, "r1")
    .or_else(|| table_get_f64(t, "d1").map(|d| d / 2.0))
    .or(r_uniform)
    .unwrap_or(1.0);

  let r2 = table_get_f64(t, "r2")
    .or_else(|| table_get_f64(t, "d2").map(|d| d / 2.0))
    .or(r_uniform)
    .unwrap_or(r1);

  // BOSL2 cyl defaults center=true (via anchor=CENTER)
  let center = table_get_bool(t, "center").unwrap_or(true);

  BoslPreviewParams::Cylinder {
    r1: r1 as f32,
    r2: r2 as f32,
    h: h as f32,
    center,
    axis,
  }
}

fn extract_sphere_preview(t: &mlua::Table) -> BoslPreviewParams {
  let r = table_get_f64(t, "r")
    .or_else(|| table_get_f64(t, "d").map(|d| d / 2.0))
    .unwrap_or(1.0);
  BoslPreviewParams::Sphere { r: r as f32 }
}

/// Extract a u32 integer from a Lua table by string key.
fn table_get_u32(t: &mlua::Table, key: &str) -> Option<u32> {
  match t.get::<LuaValue>(key).ok()? {
    LuaValue::Integer(n) => Some(n as u32),
    LuaValue::Number(n) => Some(n as u32),
    _ => None,
  }
}

/// Extract a 2-element size [w, d] from a Lua table key that holds a sub-table.
fn table_get_size2(t: &mlua::Table, key: &str) -> Option<[f64; 2]> {
  let inner = t.get::<mlua::Table>(key).ok()?;
  let w = table_get_f64_idx(&inner, 1)?;
  let d = table_get_f64_idx(&inner, 2)?;
  Some([w, d])
}

/// Extract a 3-element size [w, d, h] from the first positional arg (sub-table).
fn extract_size3_positional(t: &mlua::Table) -> Option<(f64, f64, f64)> {
  let inner = t.get::<mlua::Table>(1).ok()?;
  let w = table_get_f64_idx(&inner, 1)?;
  let d = table_get_f64_idx(&inner, 2)?;
  let h = table_get_f64_idx(&inner, 3)?;
  Some((w, d, h))
}

fn extract_tube_preview(t: &mlua::Table) -> BoslPreviewParams {
  let h = table_get_f64(t, "h")
    .or_else(|| table_get_f64(t, "l"))
    .unwrap_or(1.0);

  // Outer radius: or > od/2 > r > d/2
  let or_uniform = table_get_f64(t, "or")
    .or_else(|| table_get_f64(t, "od").map(|d| d / 2.0))
    .or_else(|| table_get_f64(t, "r"))
    .or_else(|| table_get_f64(t, "d").map(|d| d / 2.0));

  let or1 = table_get_f64(t, "or1")
    .or_else(|| table_get_f64(t, "od1").map(|d| d / 2.0))
    .or(or_uniform);

  let or2 = table_get_f64(t, "or2")
    .or_else(|| table_get_f64(t, "od2").map(|d| d / 2.0))
    .or(or_uniform);

  // Inner radius: ir > id/2
  let ir_uniform =
    table_get_f64(t, "ir").or_else(|| table_get_f64(t, "id").map(|d| d / 2.0));

  let ir1 = table_get_f64(t, "ir1")
    .or_else(|| table_get_f64(t, "id1").map(|d| d / 2.0))
    .or(ir_uniform);

  let ir2 = table_get_f64(t, "ir2")
    .or_else(|| table_get_f64(t, "id2").map(|d| d / 2.0))
    .or(ir_uniform);

  let wall = table_get_f64(t, "wall");

  // Derive missing dimensions from wall thickness
  let (or1, ir1) = match (or1, ir1, wall) {
    (Some(o), Some(i), _) => (o, i),
    (Some(o), None, Some(w)) => (o, (o - w).max(0.0)),
    (None, Some(i), Some(w)) => (i + w, i),
    (Some(o), None, None) => (o, o * 0.8), // fallback
    _ => (10.0, 8.0),                      // fallback
  };
  let (or2, ir2) = match (or2, ir2, wall) {
    (Some(o), Some(i), _) => (o, i),
    (Some(o), None, Some(w)) => (o, (o - w).max(0.0)),
    (None, Some(i), Some(w)) => (i + w, i),
    (Some(o), None, None) => (o, o * 0.8),
    _ => (or1, ir1),
  };

  let center = table_get_bool(t, "center").unwrap_or(true);

  BoslPreviewParams::Tube {
    or1: or1 as f32,
    or2: or2 as f32,
    ir1: ir1 as f32,
    ir2: ir2 as f32,
    h: h as f32,
    center,
  }
}

fn extract_torus_preview(t: &mlua::Table) -> BoslPreviewParams {
  // Method 1: r_maj + r_min (or d_maj/d_min)
  let r_maj = table_get_f64(t, "r_maj")
    .or_else(|| table_get_f64(t, "d_maj").map(|d| d / 2.0));
  let r_min = table_get_f64(t, "r_min")
    .or_else(|| table_get_f64(t, "d_min").map(|d| d / 2.0));

  if let (Some(maj), Some(min)) = (r_maj, r_min) {
    return BoslPreviewParams::Torus {
      r_maj: maj as f32,
      r_min: min as f32,
    };
  }

  // Method 2: or + ir (or od/id)
  let or_val =
    table_get_f64(t, "or").or_else(|| table_get_f64(t, "od").map(|d| d / 2.0));
  let ir_val =
    table_get_f64(t, "ir").or_else(|| table_get_f64(t, "id").map(|d| d / 2.0));

  if let (Some(outer), Some(inner)) = (or_val, ir_val) {
    return BoslPreviewParams::Torus {
      r_maj: ((outer + inner) / 2.0) as f32,
      r_min: ((outer - inner) / 2.0).abs() as f32,
    };
  }

  // Partial: r_maj with or/ir providing the other
  let maj = r_maj.unwrap_or(10.0);
  let min = r_min.unwrap_or(2.0);
  BoslPreviewParams::Torus {
    r_maj: maj as f32,
    r_min: min as f32,
  }
}

fn extract_prismoid_preview(t: &mlua::Table) -> BoslPreviewParams {
  let h = table_get_f64(t, "h")
    .or_else(|| table_get_f64(t, "l"))
    .unwrap_or(1.0);

  let size1 = table_get_size2(t, "size1").unwrap_or([1.0, 1.0]);
  let size2 = table_get_size2(t, "size2").unwrap_or(size1);

  let center = table_get_bool(t, "center").unwrap_or(true);

  BoslPreviewParams::Prismoid {
    size1: [size1[0] as f32, size1[1] as f32],
    size2: [size2[0] as f32, size2[1] as f32],
    h: h as f32,
    center,
  }
}

fn extract_rect_tube_preview(t: &mlua::Table) -> BoslPreviewParams {
  let h = table_get_f64(t, "h")
    .or_else(|| table_get_f64(t, "l"))
    .unwrap_or(1.0);

  let size = table_get_size2(t, "size").unwrap_or([10.0, 10.0]);
  let wall = table_get_f64(t, "wall");

  let isize = table_get_size2(t, "isize").unwrap_or_else(|| {
    if let Some(w) = wall {
      [(size[0] - 2.0 * w).max(0.0), (size[1] - 2.0 * w).max(0.0)]
    } else {
      [size[0] * 0.8, size[1] * 0.8]
    }
  });

  let center = table_get_bool(t, "center").unwrap_or(true);

  BoslPreviewParams::RectTube {
    size: [size[0] as f32, size[1] as f32],
    isize: [isize[0] as f32, isize[1] as f32],
    h: h as f32,
    center,
  }
}

fn extract_wedge_preview(t: &mlua::Table) -> BoslPreviewParams {
  let (w, d, h) = extract_size3_positional(t)
    .or_else(|| {
      let s = table_get_size2(t, "size");
      s.and_then(|s| {
        // wedge "size" can be [w,d,h] stored as a 3-element table
        let inner = t.get::<mlua::Table>("size").ok()?;
        let h = table_get_f64_idx(&inner, 3)?;
        Some((s[0], s[1], h))
      })
    })
    .unwrap_or((10.0, 10.0, 10.0));

  let center = table_get_bool(t, "center").unwrap_or(true);

  BoslPreviewParams::Wedge {
    w: w as f32,
    d: d as f32,
    h: h as f32,
    center,
  }
}

fn extract_octahedron_preview(t: &mlua::Table) -> BoslPreviewParams {
  let size = table_get_f64(t, "size")
    .or_else(|| table_get_f64_idx(t, 1))
    .unwrap_or(10.0);
  BoslPreviewParams::Octahedron { size: size as f32 }
}

fn extract_pie_slice_preview(t: &mlua::Table) -> BoslPreviewParams {
  let h = table_get_f64(t, "h")
    .or_else(|| table_get_f64(t, "l"))
    .unwrap_or(1.0);

  let r_uniform =
    table_get_f64(t, "r").or_else(|| table_get_f64(t, "d").map(|d| d / 2.0));

  let r1 = table_get_f64(t, "r1")
    .or_else(|| table_get_f64(t, "d1").map(|d| d / 2.0))
    .or(r_uniform)
    .unwrap_or(1.0);

  let r2 = table_get_f64(t, "r2")
    .or_else(|| table_get_f64(t, "d2").map(|d| d / 2.0))
    .or(r_uniform)
    .unwrap_or(r1);

  let ang = table_get_f64(t, "ang").unwrap_or(90.0);
  let center = table_get_bool(t, "center").unwrap_or(true);

  BoslPreviewParams::PieSlice {
    r1: r1 as f32,
    r2: r2 as f32,
    h: h as f32,
    ang: ang as f32,
    center,
  }
}

fn extract_regular_prism_preview(t: &mlua::Table) -> BoslPreviewParams {
  let n = table_get_u32(t, "n").unwrap_or(6);
  let h = table_get_f64(t, "h")
    .or_else(|| table_get_f64(t, "l"))
    .unwrap_or(1.0);

  let r_uniform =
    table_get_f64(t, "r").or_else(|| table_get_f64(t, "d").map(|d| d / 2.0));

  let r1 = table_get_f64(t, "r1")
    .or_else(|| table_get_f64(t, "d1").map(|d| d / 2.0))
    .or(r_uniform)
    .unwrap_or(1.0);

  let r2 = table_get_f64(t, "r2")
    .or_else(|| table_get_f64(t, "d2").map(|d| d / 2.0))
    .or(r_uniform)
    .unwrap_or(r1);

  let center = table_get_bool(t, "center").unwrap_or(true);

  BoslPreviewParams::RegularPrism {
    n,
    r1: r1 as f32,
    r2: r2 as f32,
    h: h as f32,
    center,
  }
}

fn extract_preview_params(
  function: &str,
  table: Option<&mlua::Table>,
) -> BoslPreviewParams {
  let Some(t) = table else {
    return BoslPreviewParams::None;
  };
  match function {
    "cuboid" => extract_cuboid_preview(t),
    "cyl" | "zcyl" => extract_cyl_preview(t, CylAxis::Z),
    "xcyl" => extract_cyl_preview(t, CylAxis::X),
    "ycyl" => extract_cyl_preview(t, CylAxis::Y),
    "spheroid" => extract_sphere_preview(t),
    "tube" => extract_tube_preview(t),
    "torus" => extract_torus_preview(t),
    "prismoid" => extract_prismoid_preview(t),
    "rect_tube" => extract_rect_tube_preview(t),
    "wedge" => extract_wedge_preview(t),
    "octahedron" => extract_octahedron_preview(t),
    "pie_slice" => extract_pie_slice_preview(t),
    "regular_prism" => extract_regular_prism_preview(t),
    "teardrop" | "onion" => extract_sphere_preview(t),
    _ => BoslPreviewParams::None,
  }
}

/// Extract preview params when a single scalar value is passed.
/// E.g. `bosl.cuboid(40)` → 40×40×40 cuboid, `bosl.spheroid(5)` → r=5.
fn extract_scalar_preview(function: &str, val: f64) -> BoslPreviewParams {
  match function {
    "cuboid" => BoslPreviewParams::Cuboid {
      w: val as f32,
      d: val as f32,
      h: val as f32,
      rounding: 0.0,
      center: true,
    },
    "cyl" | "zcyl" | "xcyl" | "ycyl" => {
      let axis = match function {
        "xcyl" => CylAxis::X,
        "ycyl" => CylAxis::Y,
        _ => CylAxis::Z,
      };
      BoslPreviewParams::Cylinder {
        r1: (val / 2.0) as f32,
        r2: (val / 2.0) as f32,
        h: val as f32,
        center: true,
        axis,
      }
    }
    "spheroid" | "teardrop" | "onion" => {
      BoslPreviewParams::Sphere { r: val as f32 }
    }
    "octahedron" => BoslPreviewParams::Octahedron { size: val as f32 },
    _ => BoslPreviewParams::None,
  }
}

/// The node a BOSL2 call records for a module that wraps children.
pub(crate) fn bosl_node_with_children(
  module: &str,
  function: &str,
  args: String,
  children: Vec<ScadNode>,
  native: Option<ScadNode>,
) -> ScadNode {
  ScadNode::BoslCall {
    module: module.to_string(),
    function: function.to_string(),
    args,
    has_children: true,
    children,
    preview: BoslPreviewParams::None,
    native: native.map(Box::new),
  }
}

/// The node a BOSL2 call records, carrying the shape built from LuaCAD's own
/// primitives when the function has a native implementation.
fn bosl_node(
  module: &str,
  function: &str,
  args: String,
  preview: BoslPreviewParams,
  native: Option<ScadNode>,
) -> ScadNode {
  ScadNode::BoslCall {
    module: module.to_string(),
    function: function.to_string(),
    args,
    has_children: false,
    children: vec![],
    preview,
    native: native.map(Box::new),
  }
}

/// Create a CsgGeometry for a BOSL2 call.
fn bosl_geometry_native(
  module: &str,
  function: &str,
  args: String,
  preview: BoslPreviewParams,
  native: Option<ScadNode>,
) -> CsgGeometry {
  CsgGeometry {
    name: None,
    // Left unmaterialized so the mesh is built from the native tree on
    // demand, rather than starting out as an empty one that never fills.
    mesh: None,
    color: None,
    scad: Some(bosl_node(module, function, args, preview, native)),
  }
}

/// Create a CsgSketch for a BOSL2 call that produces a 2D outline.
///
/// The 2D shapes have to come back as sketches rather than solids, because
/// that is what carries `linear_extrude()`, `rotate_extrude()` and `offset()`
/// — a `bosl.rect()` you cannot extrude is of no use to anyone.
fn bosl_sketch_native(
  module: &str,
  function: &str,
  args: String,
  preview: BoslPreviewParams,
  native: Option<ScadNode>,
) -> CsgSketch {
  CsgSketch {
    #[cfg(feature = "csgrs")]
    sketch: crate::geometry::empty_sketch(),
    #[cfg(not(feature = "csgrs"))]
    sketch: (),
    color: None,
    scad: Some(bosl_node(module, function, args, preview, native)),
  }
}

// ---------------------------------------------------------------------------
// Generic BOSL2 function factory
// ---------------------------------------------------------------------------

/// Create a Lua closure that wraps a BOSL2 function call.
///
/// The generated function accepts a Lua table of arguments and converts them
/// to an OpenSCAD call string. Example:
///   `bosl.cuboid { {10, 20, 30}, rounding = 2 }` → `cuboid([10, 20, 30], rounding = 2);`
fn make_bosl_fn(
  lua: &Lua,
  module: &'static str,
  function: &'static str,
) -> LuaResult<mlua::Function> {
  lua.create_function(move |lua, args: mlua::MultiValue| {
    let (scad_args, preview) = if args.is_empty() {
      (String::new(), BoslPreviewParams::None)
    } else if args.len() == 1 {
      match &args[0] {
        LuaValue::Table(t) => {
          let preview = extract_preview_params(function, Some(t));
          (lua_table_to_scad_args(t), preview)
        }
        other => {
          let preview = match other {
            LuaValue::Number(n) => extract_scalar_preview(function, *n),
            LuaValue::Integer(n) => extract_scalar_preview(function, *n as f64),
            _ => BoslPreviewParams::None,
          };
          (lua_val_to_scad(other), preview)
        }
      }
    } else {
      // Multiple positional args
      let s = args
        .iter()
        .map(lua_val_to_scad)
        .collect::<Vec<_>>()
        .join(", ");
      (s, BoslPreviewParams::None)
    };

    // Where a native builder exists it decides what actually gets rendered;
    // `scad_args` is only what the `.scad` export writes back out.
    let (native, dim) = match native_builder(function) {
      Some((params, build, dim)) => {
        let parsed = args::Args::parse(function, params, &args)?;
        (build(&parsed)?, dim)
      }
      None => (None, Dim::Solid),
    };

    match dim {
      Dim::Solid => lua.create_userdata(bosl_geometry_native(
        module, function, scad_args, preview, native,
      )),
      Dim::Sketch => lua.create_userdata(bosl_sketch_native(
        module, function, scad_args, preview, native,
      )),
    }
  })
}

/// Whether a BOSL2 function makes a solid or a flat outline.
#[derive(Clone, Copy, PartialEq)]
enum Dim {
  Solid,
  Sketch,
}

/// A native shape builder.
///
/// It returns `None` when the arguments ask for something the native
/// implementation does not cover — a textured cylinder, say — so that the
/// call falls back to OpenSCAD instead of quietly building the wrong solid.
pub type NativeBuilder = fn(&args::Args) -> LuaResult<Option<ScadNode>>;

/// The parameter list and builder for a BOSL2 shape that has a native
/// implementation, or `None` when the call always needs OpenSCAD.
fn native_builder(
  function: &str,
) -> Option<(&'static [&'static str], NativeBuilder, Dim)> {
  if let Some((params, build)) = shapes3d::builder(function) {
    return Some((params, build, Dim::Solid));
  }
  let (params, build) = shapes2d::builder(function)?;
  Some((params, build, Dim::Sketch))
}

// ---------------------------------------------------------------------------
// Module-specific registration helpers
// ---------------------------------------------------------------------------

/// The names that still go through OpenSCAD, which is now none of them.
///
/// The generic shim below is the only thing that can produce a call with no
/// native shape behind it, so recording what it registers is an exact account
/// of what is left to port — and the coverage test keeps it at zero.
static OPENSCAD_ONLY: std::sync::OnceLock<
  std::sync::Mutex<std::collections::BTreeSet<&'static str>>,
> = std::sync::OnceLock::new();

/// The clearance a printed part leaves so a matching one still fits.
///
/// BOSL2 reads this from `$slop`, which LuaCAD has no equivalent of, so the
/// same 0.0 default applies and each call can override it.
pub fn get_slop() -> f64 {
  0.0
}

/// Every name still handled by OpenSCAD, in order.
pub fn openscad_only_names() -> Vec<String> {
  OPENSCAD_ONLY
    .get()
    .and_then(|set| {
      set
        .lock()
        .ok()
        .map(|s| s.iter().map(|n| n.to_string()).collect())
    })
    .unwrap_or_default()
}

/// Register a batch of simple BOSL2 functions onto a Lua table.
fn register_functions(
  lua: &Lua,
  table: &mlua::Table,
  module: &'static str,
  names: &[&'static str],
) -> LuaResult<()> {
  let registry = OPENSCAD_ONLY
    .get_or_init(|| std::sync::Mutex::new(std::collections::BTreeSet::new()));
  for &name in names {
    let f = make_bosl_fn(lua, module, name)?;
    table.set(name, f)?;
    // A name the shim registers has no native builder, or `make_bosl_fn`
    // would have used it.
    if native_builder(name).is_none()
      && let Ok(mut set) = registry.lock()
    {
      set.insert(name);
    }
  }
  Ok(())
}

// ---------------------------------------------------------------------------
// Public API: register_bosl()
// ---------------------------------------------------------------------------

/// Register all BOSL2 functions under `bosl.*` in the given Lua state.
pub fn register_bosl(lua: &Lua) -> LuaResult<()> {
  let bosl = lua.create_table()?;

  // Modules included in BOSL2/std.scad (no extra include needed)
  register_constants(lua, &bosl)?;
  register_math(lua, &bosl)?;
  register_linalg(lua, &bosl)?;
  register_vectors(lua, &bosl)?;
  register_coords(lua, &bosl)?;
  register_lists(lua, &bosl)?;
  register_geometry(lua, &bosl)?;
  register_shapes3d(lua, &bosl)?;
  register_shapes2d(lua, &bosl)?;
  register_transforms(lua, &bosl)?;
  register_distributors(lua, &bosl)?;
  register_partitions(lua, &bosl)?;
  register_masks(lua, &bosl)?;
  register_paths(lua, &bosl)?;
  register_regions(lua, &bosl)?;
  extras::register(lua, &bosl)?;
  heightfield::register(lua, &bosl)?;
  isosurface::register(lua, &bosl)?;
  nurbs::register(lua, &bosl)?;
  metric::register(lua, &bosl)?;
  parts_extra::register(lua, &bosl)?;
  turtle::register(lua, &bosl)?;
  attachments::register(lua, &bosl)?;
  register_drawing(lua, &bosl)?;
  register_beziers(lua, &bosl)?;
  register_rounding(lua, &bosl)?;
  register_skin(lua, &bosl)?;
  register_vnf(lua, &bosl)?;

  // Modules NOT in std.scad (need separate `include <BOSL2/X.scad>`)
  register_threading(lua, &bosl)?;
  register_screws(lua, &bosl)?;
  register_screw_drive(lua, &bosl)?;
  register_gears(lua, &bosl)?;
  register_joiners(lua, &bosl)?;
  register_sliders(lua, &bosl)?;
  register_linear_bearings(lua, &bosl)?;
  register_nema_steppers(lua, &bosl)?;
  register_wiring(lua, &bosl)?;
  register_walls(lua, &bosl)?;
  register_ball_bearings(lua, &bosl)?;
  register_bottlecaps(lua, &bosl)?;
  register_cubetruss(lua, &bosl)?;
  register_hinges(lua, &bosl)?;
  register_polyhedra(lua, &bosl)?;
  register_tripod_mounts(lua, &bosl)?;

  lua.globals().set("bosl", bosl)?;
  Ok(())
}

// ===========================================================================
// constants.scad  (included via std.scad)
// ===========================================================================

fn register_constants(lua: &Lua, bosl: &mlua::Table) -> LuaResult<()> {
  // Printer slop
  bosl.set("INCH", 25.4)?;

  // Directional vectors (BOSL2 naming: no V_ prefix)
  let mk_vec = |x: f64, y: f64, z: f64| -> LuaResult<mlua::Table> {
    let t = lua.create_table()?;
    t.set(1, x)?;
    t.set(2, y)?;
    t.set(3, z)?;
    Ok(t)
  };

  bosl.set("LEFT", mk_vec(-1.0, 0.0, 0.0)?)?;
  bosl.set("RIGHT", mk_vec(1.0, 0.0, 0.0)?)?;
  bosl.set("FRONT", mk_vec(0.0, -1.0, 0.0)?)?;
  bosl.set("FWD", mk_vec(0.0, -1.0, 0.0)?)?;
  bosl.set("FORWARD", mk_vec(0.0, -1.0, 0.0)?)?;
  bosl.set("BACK", mk_vec(0.0, 1.0, 0.0)?)?;
  bosl.set("BOTTOM", mk_vec(0.0, 0.0, -1.0)?)?;
  bosl.set("BOT", mk_vec(0.0, 0.0, -1.0)?)?;
  bosl.set("DOWN", mk_vec(0.0, 0.0, -1.0)?)?;
  bosl.set("TOP", mk_vec(0.0, 0.0, 1.0)?)?;
  bosl.set("UP", mk_vec(0.0, 0.0, 1.0)?)?;
  bosl.set("CENTER", mk_vec(0.0, 0.0, 0.0)?)?;
  bosl.set("CTR", mk_vec(0.0, 0.0, 0.0)?)?;
  bosl.set("CENTRE", mk_vec(0.0, 0.0, 0.0)?)?;

  // Line specifiers
  let mk_vec2 = |a: bool, b: bool| -> LuaResult<mlua::Table> {
    let t = lua.create_table()?;
    t.set(1, a)?;
    t.set(2, b)?;
    Ok(t)
  };
  bosl.set("SEGMENT", mk_vec2(true, true)?)?;
  bosl.set("RAY", mk_vec2(true, false)?)?;
  bosl.set("LINE", mk_vec2(false, false)?)?;

  // Identity matrix
  let ident = lua.create_table()?;
  let row1 = lua.create_table()?;
  row1.set(1, 1.0)?;
  row1.set(2, 0.0)?;
  row1.set(3, 0.0)?;
  row1.set(4, 0.0)?;
  let row2 = lua.create_table()?;
  row2.set(1, 0.0)?;
  row2.set(2, 1.0)?;
  row2.set(3, 0.0)?;
  row2.set(4, 0.0)?;
  let row3 = lua.create_table()?;
  row3.set(1, 0.0)?;
  row3.set(2, 0.0)?;
  row3.set(3, 1.0)?;
  row3.set(4, 0.0)?;
  let row4 = lua.create_table()?;
  row4.set(1, 0.0)?;
  row4.set(2, 0.0)?;
  row4.set(3, 0.0)?;
  row4.set(4, 1.0)?;
  ident.set(1, row1)?;
  ident.set(2, row2)?;
  ident.set(3, row3)?;
  ident.set(4, row4)?;
  bosl.set("IDENT", ident)?;

  // BOSL2 reads the printer's clearance from `$slop`, which LuaCAD has no
  // equivalent of, so this reports the same default every call falls back to.
  bosl.set("get_slop", lua.create_function(|_, ()| Ok(get_slop()))?)?;

  Ok(())
}

// ===========================================================================
// math.scad  (included via std.scad)
// ===========================================================================

fn register_math(lua: &Lua, bosl: &mlua::Table) -> LuaResult<()> {
  math::register(lua, bosl)
}

// ===========================================================================
// linalg.scad  (included via std.scad)
// ===========================================================================

fn register_linalg(lua: &Lua, bosl: &mlua::Table) -> LuaResult<()> {
  linalg::register(lua, bosl)
}

// ===========================================================================
// vectors.scad  (included via std.scad)
// ===========================================================================

fn register_vectors(lua: &Lua, bosl: &mlua::Table) -> LuaResult<()> {
  vectors::register(lua, bosl)
}

// ===========================================================================
// coords.scad  (included via std.scad)
// ===========================================================================

fn register_coords(lua: &Lua, bosl: &mlua::Table) -> LuaResult<()> {
  coords::register(lua, bosl)
}

// ===========================================================================
// lists.scad  (included via std.scad)
// ===========================================================================

fn register_lists(lua: &Lua, bosl: &mlua::Table) -> LuaResult<()> {
  lists::register(lua, bosl)
}

// ===========================================================================
// geometry.scad  (included via std.scad)
// ===========================================================================

fn register_geometry(lua: &Lua, bosl: &mlua::Table) -> LuaResult<()> {
  geom::register(lua, bosl)
}

// ===========================================================================
// shapes3d.scad  (included via std.scad)
// ===========================================================================

fn register_shapes3d(lua: &Lua, bosl: &mlua::Table) -> LuaResult<()> {
  register_functions(
    lua,
    bosl,
    "std.scad",
    &[
      // Cuboids, prismoids, pyramids
      "cuboid",
      "prismoid",
      "regular_prism",
      "rect_tube",
      "wedge",
      "octahedron",
      // Cylinders
      "cyl",
      "xcyl",
      "ycyl",
      "zcyl",
      "tube",
      "pie_slice",
      // Other round objects
      "spheroid",
      "torus",
      "teardrop",
      "onion",
      // Text
      "text3d",
      "path_text",
      // Miscellaneous
      "fillet",
      "interior_fillet",
    ],
  )?;

  Ok(())
}

// ===========================================================================
// shapes2d.scad  (included via std.scad)
// ===========================================================================

fn register_shapes2d(lua: &Lua, bosl: &mlua::Table) -> LuaResult<()> {
  register_functions(
    lua,
    bosl,
    "std.scad",
    &[
      // 2D primitives
      "rect",
      "ellipse",
      // Polygons
      "regular_ngon",
      "pentagon",
      "hexagon",
      "octagon",
      "right_triangle",
      "trapezoid",
      "star",
      "jittered_poly",
      // Curved 2D shapes
      "teardrop2d",
      "egg",
      "ring",
      "glued_circles",
      "squircle",
      "keyhole",
      "reuleaux_polygon",
      "supershape",
    ],
  )?;

  // The two 2D operators take a shape rather than only numbers, which the
  // generic shim has no way to express.
  shapes2d::register(lua, bosl)?;

  Ok(())
}

// ===========================================================================
// transforms.scad  (included via std.scad)
// ===========================================================================

fn register_transforms(lua: &Lua, bosl: &mlua::Table) -> LuaResult<()> {
  transforms::register(lua, bosl)
}

// ===========================================================================
// distributors.scad  (included via std.scad)
// ===========================================================================

fn register_distributors(lua: &Lua, bosl: &mlua::Table) -> LuaResult<()> {
  distributors::register(lua, bosl)
}

// ===========================================================================
// partitions.scad  (included via std.scad)
// ===========================================================================

fn register_partitions(lua: &Lua, bosl: &mlua::Table) -> LuaResult<()> {
  masks::register(lua, bosl)
}

// ===========================================================================
// masks.scad  (included via std.scad)
// ===========================================================================

fn register_masks(_lua: &Lua, _bosl: &mlua::Table) -> LuaResult<()> {
  // Registered with the partitions, which share their machinery.
  Ok(())
}

// ===========================================================================
// paths.scad  (included via std.scad)
// ===========================================================================

fn register_paths(lua: &Lua, bosl: &mlua::Table) -> LuaResult<()> {
  paths::register(lua, bosl)
}

// ===========================================================================
// regions.scad  (included via std.scad)
// ===========================================================================

fn register_regions(lua: &Lua, bosl: &mlua::Table) -> LuaResult<()> {
  regions::register(lua, bosl)
}

// ===========================================================================
// drawing.scad  (included via std.scad)
// ===========================================================================

fn register_drawing(lua: &Lua, bosl: &mlua::Table) -> LuaResult<()> {
  sweeps::register(lua, bosl)
}

// ===========================================================================
// beziers.scad  (included via std.scad)
// ===========================================================================

fn register_beziers(lua: &Lua, bosl: &mlua::Table) -> LuaResult<()> {
  beziers::register(lua, bosl)
}

// ===========================================================================
// rounding.scad  (included via std.scad)
// ===========================================================================

fn register_rounding(lua: &Lua, bosl: &mlua::Table) -> LuaResult<()> {
  // The sweeps themselves are registered together with the rest of their
  // machinery; this is the end-treatment specs they are steered by.
  rounding::register(lua, bosl)
}

// ===========================================================================
// skin.scad  (included via std.scad)
// ===========================================================================

fn register_skin(lua: &Lua, bosl: &mlua::Table) -> LuaResult<()> {
  // The sweeps themselves are registered with the rest of their machinery;
  // this is the texture catalogue they tile a surface with.
  textures::register(lua, bosl)
}

// ===========================================================================
// vnf.scad  (included via std.scad)
// ===========================================================================

fn register_vnf(lua: &Lua, bosl: &mlua::Table) -> LuaResult<()> {
  vnf_lua::register(lua, bosl)
}

// ===========================================================================
// threading.scad  (NOT in std.scad — needs separate include)
// ===========================================================================

fn register_threading(lua: &Lua, bosl: &mlua::Table) -> LuaResult<()> {
  threading::register(lua, bosl)
}

// ===========================================================================
// screws.scad  (NOT in std.scad)
// ===========================================================================

fn register_screws(lua: &Lua, bosl: &mlua::Table) -> LuaResult<()> {
  screws::register(lua, bosl)
}

// ===========================================================================
// screw_drive.scad  (NOT in std.scad)
// ===========================================================================

fn register_screw_drive(_lua: &Lua, _bosl: &mlua::Table) -> LuaResult<()> {
  // Registered with the threads, which share their tables.
  Ok(())
}

// ===========================================================================
// gears.scad  (NOT in std.scad)
// ===========================================================================

fn register_gears(lua: &Lua, bosl: &mlua::Table) -> LuaResult<()> {
  gears::register(lua, bosl)
}

// ===========================================================================
// joiners.scad  (NOT in std.scad)
// ===========================================================================

fn register_joiners(lua: &Lua, bosl: &mlua::Table) -> LuaResult<()> {
  parts::register(lua, bosl)
}

// ===========================================================================
// sliders.scad  (NOT in std.scad)
// ===========================================================================

fn register_sliders(_lua: &Lua, _bosl: &mlua::Table) -> LuaResult<()> {
  // Registered with the other mechanical parts, which share their tables.
  Ok(())
}

// ===========================================================================
// linear_bearings.scad  (NOT in std.scad)
// ===========================================================================

fn register_linear_bearings(_lua: &Lua, _bosl: &mlua::Table) -> LuaResult<()> {
  // Registered with the other mechanical parts, which share their tables.
  Ok(())
}

// ===========================================================================
// nema_steppers.scad  (NOT in std.scad)
// ===========================================================================

fn register_nema_steppers(_lua: &Lua, _bosl: &mlua::Table) -> LuaResult<()> {
  // Registered with the other mechanical parts, which share their tables.
  Ok(())
}

// ===========================================================================
// wiring.scad  (NOT in std.scad)
// ===========================================================================

fn register_wiring(_lua: &Lua, _bosl: &mlua::Table) -> LuaResult<()> {
  // Registered with the other mechanical parts, which share their tables.
  Ok(())
}

// ===========================================================================
// walls.scad  (NOT in std.scad)
// ===========================================================================

fn register_walls(_lua: &Lua, _bosl: &mlua::Table) -> LuaResult<()> {
  // Registered with the other mechanical parts, which share their tables.
  Ok(())
}

// ===========================================================================
// ball_bearings.scad  (NOT in std.scad)
// ===========================================================================

fn register_ball_bearings(_lua: &Lua, _bosl: &mlua::Table) -> LuaResult<()> {
  // Registered with the other mechanical parts, which share their tables.
  Ok(())
}

// ===========================================================================
// bottlecaps.scad  (NOT in std.scad)
// ===========================================================================

fn register_bottlecaps(_lua: &Lua, _bosl: &mlua::Table) -> LuaResult<()> {
  // Registered with the other mechanical parts, which share their tables.
  Ok(())
}

// ===========================================================================
// cubetruss.scad  (NOT in std.scad)
// ===========================================================================

fn register_cubetruss(_lua: &Lua, _bosl: &mlua::Table) -> LuaResult<()> {
  // Registered with the other mechanical parts, which share their tables.
  Ok(())
}

// ===========================================================================
// hinges.scad  (NOT in std.scad)
// ===========================================================================

fn register_hinges(_lua: &Lua, _bosl: &mlua::Table) -> LuaResult<()> {
  // Registered with the other mechanical parts, which share their tables.
  Ok(())
}

// ===========================================================================
// polyhedra.scad  (NOT in std.scad)
// ===========================================================================

fn register_polyhedra(_lua: &Lua, _bosl: &mlua::Table) -> LuaResult<()> {
  // Registered with the other mechanical parts, which share their tables.
  Ok(())
}

// ===========================================================================
// tripod_mounts.scad  (NOT in std.scad)
// ===========================================================================

fn register_tripod_mounts(_lua: &Lua, _bosl: &mlua::Table) -> LuaResult<()> {
  // Registered with the other mechanical parts, which share their tables.
  Ok(())
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
  use super::*;

  fn run_bosl_lua(code: &str) -> Vec<ScadNode> {
    let lua = Lua::new();
    register_bosl(&lua).expect("Failed to register BOSL2");

    // Register a minimal render() to collect geometries
    let collector =
      std::rc::Rc::new(std::cell::RefCell::new(Vec::<CsgGeometry>::new()));
    let collector_clone = collector.clone();
    let render_fn = lua
      .create_function(move |_, ud: mlua::AnyUserData| {
        let geom = ud.borrow::<CsgGeometry>()?.clone();
        collector_clone.borrow_mut().push(geom);
        Ok(())
      })
      .unwrap();
    lua.globals().set("render", render_fn).unwrap();

    let result: mlua::MultiValue =
      lua.load(code).eval().expect("Lua eval failed");

    // Collect returned geometries
    let mut nodes = Vec::new();
    for val in result.iter() {
      if let LuaValue::UserData(ud) = val
        && let Ok(geom) = ud.borrow::<CsgGeometry>()
        && let Some(ref scad) = geom.scad
      {
        nodes.push(scad.clone());
      }
    }

    // Also collect rendered geometries
    for geom in collector.borrow().iter() {
      if let Some(ref scad) = geom.scad {
        nodes.push(scad.clone());
      }
    }

    nodes
  }

  #[test]
  fn bosl_cuboid_basic() {
    let nodes = run_bosl_lua("return bosl.cuboid { {10, 20, 30} }");
    assert_eq!(nodes.len(), 1);
    if let ScadNode::BoslCall {
      module,
      function,
      args,
      ..
    } = &nodes[0]
    {
      assert_eq!(module, "std.scad");
      assert_eq!(function, "cuboid");
      assert!(args.contains("[10, 20, 30]"));
    } else {
      panic!("Expected BoslCall, got {:?}", nodes[0]);
    }
  }

  #[test]
  fn bosl_cuboid_with_named_args() {
    let nodes = run_bosl_lua(
      "return bosl.cuboid { {10, 20, 30}, rounding = 2, trimcorners = true }",
    );
    assert_eq!(nodes.len(), 1);
    if let ScadNode::BoslCall { args, .. } = &nodes[0] {
      assert!(args.contains("[10, 20, 30]"));
      assert!(args.contains("trimcorners = true"));
      assert!(args.contains("rounding = 2"));
    } else {
      panic!("Expected BoslCall");
    }
  }

  #[test]
  fn bosl_threaded_rod() {
    let nodes =
      run_bosl_lua("return bosl.threaded_rod { d = 10, l = 30, pitch = 2 }");
    assert_eq!(nodes.len(), 1);
    if let ScadNode::BoslCall {
      module, function, ..
    } = &nodes[0]
    {
      assert_eq!(module, "threading.scad");
      assert_eq!(function, "threaded_rod");
    } else {
      panic!("Expected BoslCall");
    }
  }

  #[test]
  fn bosl_scad_generation() {
    let nodes = run_bosl_lua("return bosl.cuboid { {10, 20, 30} }");
    let scad = crate::scad_export::generate_scad(&nodes);
    assert!(
      scad.contains("include <BOSL2/std.scad>"),
      "Missing BOSL2 std include: {}",
      scad
    );
    assert!(
      scad.contains("cuboid([10, 20, 30]);"),
      "Missing cuboid call: {}",
      scad
    );
  }

  #[test]
  fn bosl_constants_available() {
    let lua = Lua::new();
    register_bosl(&lua).expect("Failed to register BOSL2");
    let phi: f64 = lua
      .load("return bosl.PHI")
      .eval()
      .expect("Failed to get PHI");
    let expected = (1.0_f64 + 5.0_f64.sqrt()) / 2.0;
    assert!((phi - expected).abs() < 1e-10);
  }

  #[test]
  fn bosl_vector_constants() {
    let lua = Lua::new();
    register_bosl(&lua).expect("Failed to register BOSL2");
    let val: f64 = lua
      .load("return bosl.UP[3]")
      .eval()
      .expect("Failed to get UP[3]");
    assert_eq!(val, 1.0);
  }

  #[test]
  fn bosl_spur_gear_function() {
    let nodes = run_bosl_lua(
      "return bosl.spur_gear { circ_pitch = 5, teeth = 20, thickness = 5 }",
    );
    assert_eq!(nodes.len(), 1);
    if let ScadNode::BoslCall {
      module, function, ..
    } = &nodes[0]
    {
      assert_eq!(module, "gears.scad");
      assert_eq!(function, "spur_gear");
    } else {
      panic!("Expected BoslCall");
    }
  }

  #[test]
  fn bosl_multiple_modules_scad() {
    let nodes = run_bosl_lua(
      r#"
      render(bosl.cuboid { {10, 10, 10} })
      return bosl.threaded_rod { d = 10, l = 30, pitch = 2 }
      "#,
    );
    let scad = crate::scad_export::generate_scad(&nodes);
    assert!(scad.contains("include <BOSL2/std.scad>"));
    assert!(scad.contains("include <BOSL2/threading.scad>"));
  }

  #[test]
  fn bosl_cuboid_preview_nested_table() {
    let nodes = run_bosl_lua("return bosl.cuboid { {10, 20, 30} }");
    if let ScadNode::BoslCall { preview, .. } = &nodes[0] {
      match preview {
        BoslPreviewParams::Cuboid {
          w,
          d,
          h,
          rounding,
          center,
        } => {
          assert_eq!(*w, 10.0);
          assert_eq!(*d, 20.0);
          assert_eq!(*h, 30.0);
          assert_eq!(*rounding, 0.0);
          assert!(*center); // BOSL2 default
        }
        other => panic!("Expected Cuboid preview, got {:?}", other),
      }
    } else {
      panic!("Expected BoslCall");
    }
  }

  #[test]
  fn bosl_cuboid_preview_flat_table() {
    // User calling convention: bosl.cuboid({40,40,40})
    let nodes = run_bosl_lua("return bosl.cuboid({40, 40, 40})");
    if let ScadNode::BoslCall { preview, .. } = &nodes[0] {
      match preview {
        BoslPreviewParams::Cuboid {
          w,
          d,
          h,
          rounding,
          center,
        } => {
          assert_eq!(*w, 40.0);
          assert_eq!(*d, 40.0);
          assert_eq!(*h, 40.0);
          assert_eq!(*rounding, 0.0);
          assert!(*center);
        }
        other => panic!("Expected Cuboid preview, got {:?}", other),
      }
    } else {
      panic!("Expected BoslCall");
    }
  }

  /// BOSL2's `cuboid()` has no `center` parameter — it is always centred
  /// unless an anchor says otherwise. OpenSCAD would drop the argument in
  /// silence and build a centred cuboid anyway, so the script would be
  /// quietly wrong; saying so is more useful.
  #[test]
  fn bosl_cuboid_rejects_center_and_points_at_anchor() {
    let lua = Lua::new();
    register_bosl(&lua).unwrap();
    let err = lua
      .load("return bosl.cuboid { {10, 20, 30}, center = false }")
      .eval::<mlua::Value>()
      .unwrap_err()
      .to_string();
    assert!(err.contains("unknown parameter 'center'"), "{err}");
    assert!(err.contains("anchor = bosl.BOTTOM"), "{err}");
  }

  #[test]
  fn bosl_cyl_preview_diameter() {
    let nodes = run_bosl_lua("return bosl.cyl { d = 10, h = 20 }");
    if let ScadNode::BoslCall { preview, .. } = &nodes[0] {
      match preview {
        BoslPreviewParams::Cylinder {
          r1, r2, h, center, ..
        } => {
          assert_eq!(*r1, 5.0);
          assert_eq!(*r2, 5.0);
          assert_eq!(*h, 20.0);
          assert!(*center);
        }
        other => panic!("Expected Cylinder preview, got {:?}", other),
      }
    } else {
      panic!("Expected BoslCall");
    }
  }

  #[test]
  fn bosl_cyl_preview_tapered() {
    let nodes = run_bosl_lua("return bosl.cyl { r1 = 5, r2 = 10, h = 20 }");
    if let ScadNode::BoslCall { preview, .. } = &nodes[0] {
      match preview {
        BoslPreviewParams::Cylinder { r1, r2, h, .. } => {
          assert_eq!(*r1, 5.0);
          assert_eq!(*r2, 10.0);
          assert_eq!(*h, 20.0);
        }
        other => panic!("Expected Cylinder preview, got {:?}", other),
      }
    } else {
      panic!("Expected BoslCall");
    }
  }

  #[test]
  fn bosl_xcyl_preview_axis() {
    let nodes = run_bosl_lua("return bosl.xcyl { r = 5, h = 20 }");
    if let ScadNode::BoslCall { preview, .. } = &nodes[0] {
      match preview {
        BoslPreviewParams::Cylinder { axis, .. } => {
          assert!(matches!(axis, CylAxis::X));
        }
        other => panic!("Expected Cylinder preview, got {:?}", other),
      }
    } else {
      panic!("Expected BoslCall");
    }
  }

  #[test]
  fn bosl_cuboid_scalar_preview() {
    // bosl.cuboid(40) should produce a 40×40×40 cuboid preview
    let nodes = run_bosl_lua("return bosl.cuboid(40)");
    assert_eq!(nodes.len(), 1);
    if let ScadNode::BoslCall { preview, args, .. } = &nodes[0] {
      assert_eq!(args, "40");
      match preview {
        BoslPreviewParams::Cuboid {
          w,
          d,
          h,
          rounding,
          center,
        } => {
          assert_eq!(*w, 40.0);
          assert_eq!(*d, 40.0);
          assert_eq!(*h, 40.0);
          assert_eq!(*rounding, 0.0);
          assert!(*center);
        }
        other => panic!("Expected Cuboid preview, got {:?}", other),
      }
    } else {
      panic!("Expected BoslCall");
    }
  }
}
