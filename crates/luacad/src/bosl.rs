/// BOSL2 (Belfry OpenSCAD Library v2) function bindings for LuaCAD.
///
/// All functions are registered under the `bosl` Lua namespace table so they
/// can be called as `bosl.cuboid(...)`, `bosl.cyl(...)`, etc.
///
/// Because BOSL2 is an OpenSCAD library, these functions produce ScadNode-only
/// geometry (no mesh). The generated SCAD output automatically includes the
/// required `include <BOSL2/std.scad>` directive and any additional module
/// includes for non-std modules (threading, gears, screws, etc.).
#[cfg(feature = "csgrs")]
use csgrs::mesh::Mesh as CsgMesh;
#[cfg(feature = "csgrs")]
use csgrs::traits::CSG;
use mlua::{Lua, Result as LuaResult, Value as LuaValue};

use crate::geometry::CsgGeometry;
use crate::scad_export::{BoslPreviewParams, CylAxis, ScadNode};

// ---------------------------------------------------------------------------
// Helpers: Lua value → OpenSCAD argument string
// ---------------------------------------------------------------------------

/// Format a Lua value as an OpenSCAD argument string fragment.
fn lua_val_to_scad(v: &LuaValue) -> String {
  match v {
    LuaValue::Number(n) => format_f64(*n),
    LuaValue::Integer(n) => n.to_string(),
    LuaValue::Boolean(b) => b.to_string(),
    LuaValue::String(s) => {
      let s = s.to_str().map(|s| s.to_string()).unwrap_or_default();
      format!("\"{}\"", s)
    }
    LuaValue::Table(t) => lua_table_to_scad_array(t),
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
/// Only processes sequential integer keys (the array portion).
fn lua_table_to_scad_array(t: &mlua::Table) -> String {
  let len = t.len().unwrap_or(0);
  if len == 0 {
    return "[]".to_string();
  }
  let mut parts = Vec::new();
  for i in 1..=len {
    if let Ok(v) = t.get::<LuaValue>(i) {
      parts.push(lua_val_to_scad(&v));
    }
  }
  format!("[{}]", parts.join(", "))
}

/// Convert a Lua arguments table `{ positional..., named_key = val, ... }`
/// into an OpenSCAD argument string like `[10, 20], fillet = 2, center = true`.
///
/// Positional (integer-keyed) values are emitted first, then named keys
/// in alphabetical order.
fn lua_table_to_scad_args(t: &mlua::Table) -> String {
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
  let y = table_get_f64_idx(t, 2)?;
  let z = table_get_f64_idx(t, 3)?;
  Some((x, y, z))
}

fn extract_cuboid_preview(t: &mlua::Table) -> BoslPreviewParams {
  let (w, d, h) = extract_cuboid_size(t).unwrap_or((1.0, 1.0, 1.0));
  // BOSL2 cuboid defaults center=true (via anchor=CENTER)
  let center = table_get_bool(t, "center").unwrap_or(true);
  BoslPreviewParams::Cuboid {
    w: w as f32,
    d: d as f32,
    h: h as f32,
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
    "spheroid" => BoslPreviewParams::Sphere { r: val as f32 },
    _ => BoslPreviewParams::None,
  }
}

/// Create a CsgGeometry representing a BOSL2 function call.
fn bosl_geometry(
  module: &str,
  function: &str,
  args: String,
  preview: BoslPreviewParams,
) -> CsgGeometry {
  CsgGeometry {
    mesh: {
      #[cfg(feature = "csgrs")]
      {
        Some(CsgMesh::<()>::new())
      }
      #[cfg(not(feature = "csgrs"))]
      {
        None
      }
    },
    color: None,
    scad: Some(ScadNode::BoslCall {
      module: module.to_string(),
      function: function.to_string(),
      args,
      has_children: false,
      children: vec![],
      preview,
    }),
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
  lua.create_function(move |_, args: mlua::MultiValue| {
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
    Ok(bosl_geometry(module, function, scad_args, preview))
  })
}

// ---------------------------------------------------------------------------
// Module-specific registration helpers
// ---------------------------------------------------------------------------

/// Register a batch of simple BOSL2 functions onto a Lua table.
fn register_functions(
  lua: &Lua,
  table: &mlua::Table,
  module: &'static str,
  names: &[&'static str],
) -> LuaResult<()> {
  for &name in names {
    let f = make_bosl_fn(lua, module, name)?;
    table.set(name, f)?;
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

  register_functions(lua, bosl, "std.scad", &["get_slop"])?;

  Ok(())
}

// ===========================================================================
// math.scad  (included via std.scad)
// ===========================================================================

fn register_math(lua: &Lua, bosl: &mlua::Table) -> LuaResult<()> {
  // Constants
  bosl.set("PHI", (1.0_f64 + 5.0_f64.sqrt()) / 2.0)?;
  bosl.set("EPSILON", 1e-9_f64)?;

  register_functions(
    lua,
    bosl,
    "std.scad",
    &[
      // Interpolation and counting
      "count",
      "lerp",
      "lerpn",
      "bilerp",
      "slerp",
      "slerpn",
      // Miscellaneous functions
      "sqr",
      "log2",
      "hypot",
      "factorial",
      "binomial",
      "binomial_coefficient",
      "gcd",
      "lcm",
      // Hyperbolic trigonometry
      "sinh",
      "cosh",
      "tanh",
      "asinh",
      "acosh",
      "atanh",
      // Quantization
      "quant",
      "quantdn",
      "quantup",
      // Constraints and modulos
      "constrain",
      "posmod",
      "modang",
      // Operations on lists
      "sum",
      "mean",
      "median",
      "deltas",
      "cumsum",
      "product",
      "cumprod",
      "convolve",
      "sum_of_sines",
      // Random number generation
      "rand_int",
      "random_points",
      "gaussian_rands",
      "exponential_rands",
      "spherical_random_points",
      "random_polygon",
      // Calculus
      "deriv",
      "deriv2",
      "deriv3",
      // Complex numbers
      "complex",
      "c_mul",
      "c_div",
      "c_conj",
      "c_real",
      "c_imag",
      "c_ident",
      "c_norm",
      // Polynomials
      "quadratic_roots",
      "polynomial",
      "poly_mult",
      "poly_div",
      "poly_add",
      "poly_roots",
      "real_roots",
      // Root finding
      "root_find",
    ],
  )?;

  Ok(())
}

// ===========================================================================
// linalg.scad  (included via std.scad)
// ===========================================================================

fn register_linalg(lua: &Lua, bosl: &mlua::Table) -> LuaResult<()> {
  register_functions(
    lua,
    bosl,
    "std.scad",
    &[
      // Matrix testing
      "is_matrix",
      "is_matrix_symmetric",
      "is_rotation",
      "echo_matrix",
      // Matrix indexing
      "column",
      "submatrix",
      // Matrix construction
      "ident",
      "diagonal_matrix",
      "transpose",
      "outer_product",
      "submatrix_set",
      "hstack",
      "block_matrix",
      // Solving and factorization
      "linear_solve",
      "linear_solve3",
      "matrix_inverse",
      "rot_inverse",
      "null_space",
      "qr_factor",
      "back_substitute",
      "cholesky",
      // Matrix properties
      "det2",
      "det3",
      "det4",
      "determinant",
      "norm_fro",
      "matrix_trace",
    ],
  )?;

  Ok(())
}

// ===========================================================================
// vectors.scad  (included via std.scad)
// ===========================================================================

fn register_vectors(lua: &Lua, bosl: &mlua::Table) -> LuaResult<()> {
  register_functions(
    lua,
    bosl,
    "std.scad",
    &[
      // Vector testing
      "is_vector",
      // Scalar operations on vectors
      "add_scalar",
      "v_mul",
      "v_div",
      "v_abs",
      "v_ceil",
      "v_floor",
      "v_round",
      "v_lookup",
      // Vector properties
      "unit",
      "v_theta",
      "vector_angle",
      "vector_axis",
      "vector_bisect",
      "vector_perp",
      // Searching
      "closest_point",
      "furthest_point",
      "vector_search",
      "vector_search_tree",
      "vector_nearest",
      // Bounds
      "pointlist_bounds",
      "fit_to_box",
    ],
  )?;

  Ok(())
}

// ===========================================================================
// coords.scad  (included via std.scad)
// ===========================================================================

fn register_coords(lua: &Lua, bosl: &mlua::Table) -> LuaResult<()> {
  register_functions(
    lua,
    bosl,
    "std.scad",
    &[
      // Coordinate manipulation
      "point2d",
      "path2d",
      "point3d",
      "path3d",
      "point4d",
      "path4d",
      // Coordinate systems
      "polar_to_xy",
      "xy_to_polar",
      "project_plane",
      "lift_plane",
      "cylindrical_to_xyz",
      "xyz_to_cylindrical",
      "spherical_to_xyz",
      "xyz_to_spherical",
      "altaz_to_xyz",
      "xyz_to_altaz",
    ],
  )?;

  Ok(())
}

// ===========================================================================
// lists.scad  (included via std.scad)
// ===========================================================================

fn register_lists(lua: &Lua, bosl: &mlua::Table) -> LuaResult<()> {
  register_functions(
    lua,
    bosl,
    "std.scad",
    &[
      // List query
      "is_homogeneous",
      "min_length",
      "max_length",
      "list_shape",
      "in_list",
      // List indexing
      "select",
      "slice",
      "last",
      "list_head",
      "list_tail",
      "bselect",
      // List construction
      "repeat",
      "list_bset",
      "list",
      "force_list",
      // List modification
      "reverse",
      "list_rotate",
      "shuffle",
      "repeat_entries",
      "list_pad",
      "list_set",
      "list_insert",
      "list_remove",
      "list_remove_values",
      // Iteration helpers
      "idx",
      // Subsets
      "pair",
      "triplet",
      "combinations",
      "permutations",
      // Structure
      "list_to_matrix",
      "flatten",
      "full_flatten",
      // Set operations
      "set_union",
      "set_difference",
      "set_intersection",
    ],
  )?;

  Ok(())
}

// ===========================================================================
// geometry.scad  (included via std.scad)
// ===========================================================================

fn register_geometry(lua: &Lua, bosl: &mlua::Table) -> LuaResult<()> {
  register_functions(
    lua,
    bosl,
    "std.scad",
    &[
      // Lines, rays, segments
      "is_point_on_line",
      "is_collinear",
      "point_line_distance",
      "segment_distance",
      "line_normal",
      "line_intersection",
      "line_closest_point",
      "line_from_points",
      // Planes
      "is_coplanar",
      "plane3pt",
      "plane3pt_indexed",
      "plane_from_normal",
      "plane_from_points",
      "plane_from_polygon",
      "plane_normal",
      "plane_offset",
      "plane_line_intersection",
      "plane_intersection",
      "plane_line_angle",
      "plane_closest_point",
      "point_plane_distance",
      "are_points_on_plane",
      // Circle calculations
      "circle_line_intersection",
      "circle_circle_intersection",
      "circle_2tangents",
      "circle_3points",
      "circle_point_tangents",
      "circle_circle_tangents",
      // Sphere calculations
      "sphere_line_intersection",
      // Polygons
      "polygon_area",
      "centroid",
      "polygon_normal",
      "point_in_polygon",
      "polygon_line_intersection",
      "polygon_triangulate",
      "is_polygon_clockwise",
      "clockwise_polygon",
      "ccw_polygon",
      "reverse_polygon",
      "reindex_polygon",
      "align_polygon",
      "are_polygons_equal",
      // Convex hull
      "hull2d_path",
      "hull3d_faces",
      // Convex sets
      "is_polygon_convex",
      "convex_distance",
      "convex_collision",
      // Rotation decoding
      "rot_decode",
    ],
  )?;

  Ok(())
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
      // Rounding 2D shapes
      "round2d",
      "shell2d",
    ],
  )?;

  Ok(())
}

// ===========================================================================
// transforms.scad  (included via std.scad)
// ===========================================================================

fn register_transforms(lua: &Lua, bosl: &mlua::Table) -> LuaResult<()> {
  register_functions(
    lua,
    bosl,
    "std.scad",
    &[
      // Translations
      "move",
      "left",
      "right",
      "xmove",
      "fwd",
      "back",
      "ymove",
      "down",
      "up",
      "zmove",
      // Rotations
      "rot",
      "xrot",
      "yrot",
      "zrot",
      "tilt",
      // Scaling
      "xscale",
      "yscale",
      "zscale",
      // Reflection/mirroring
      "xflip",
      "yflip",
      "zflip",
      // Other
      "frame_map",
      "skew",
      "apply",
    ],
  )?;

  Ok(())
}

// ===========================================================================
// distributors.scad  (included via std.scad)
// ===========================================================================

fn register_distributors(lua: &Lua, bosl: &mlua::Table) -> LuaResult<()> {
  register_functions(
    lua,
    bosl,
    "std.scad",
    &[
      // Translating copies
      "move_copies",
      "xcopies",
      "ycopies",
      "zcopies",
      "line_copies",
      "grid_copies",
      // Rotating copies
      "rot_copies",
      "xrot_copies",
      "yrot_copies",
      "zrot_copies",
      "arc_copies",
      "sphere_copies",
      // Path-based placement
      "path_copies",
      // Mirroring/reflection
      "xflip_copy",
      "yflip_copy",
      "zflip_copy",
      "mirror_copy",
      // Distribution
      "xdistribute",
      "ydistribute",
      "zdistribute",
      "distribute",
    ],
  )?;

  Ok(())
}

// ===========================================================================
// partitions.scad  (included via std.scad)
// ===========================================================================

fn register_partitions(lua: &Lua, bosl: &mlua::Table) -> LuaResult<()> {
  register_functions(
    lua,
    bosl,
    "std.scad",
    &[
      // Planar cutting
      "half_of",
      "left_half",
      "right_half",
      "front_half",
      "back_half",
      "bottom_half",
      "top_half",
      // Interlocking partitions
      "partition_mask",
      "partition_cut_mask",
      "partition",
    ],
  )?;

  Ok(())
}

// ===========================================================================
// masks.scad  (included via std.scad)
// ===========================================================================

fn register_masks(lua: &Lua, bosl: &mlua::Table) -> LuaResult<()> {
  register_functions(
    lua,
    bosl,
    "std.scad",
    &[
      // 2D masking shapes
      "mask2d_roundover",
      "mask2d_smooth",
      "mask2d_teardrop",
      "mask2d_cove",
      "mask2d_chamfer",
      "mask2d_rabbet",
      "mask2d_dovetail",
      "mask2d_ogee",
      // 2D mask application
      "face_profile",
      "edge_profile",
      "edge_profile_asym",
      "corner_profile",
      // 3D edge masks
      "chamfer_edge_mask",
      "rounding_edge_mask",
      "teardrop_edge_mask",
      "polygon_edge_mask",
      // 3D corner masks
      "chamfer_corner_mask",
      "rounding_corner_mask",
      "teardrop_corner_mask",
      // 3D cylinder masks
      "chamfer_cylinder_mask",
      "rounding_cylinder_mask",
      // 3D cylindrical hole masks
      "rounding_hole_mask",
      // 3D mask application
      "face_mask",
      "edge_mask",
      "corner_mask",
    ],
  )?;

  Ok(())
}

// ===========================================================================
// paths.scad  (included via std.scad)
// ===========================================================================

fn register_paths(lua: &Lua, bosl: &mlua::Table) -> LuaResult<()> {
  register_functions(
    lua,
    bosl,
    "std.scad",
    &[
      "path_length",
      "path_segment_lengths",
      "path_closest_point",
      "path_tangent",
      "path_normal",
      "path_cut",
      "path_cut_points",
      "subdivide_path",
      "resample_path",
      "is_path",
      "is_1region",
      "force_path",
      "force_region",
      "path_merge_collinear",
    ],
  )?;

  Ok(())
}

// ===========================================================================
// drawing.scad  (included via std.scad)
// ===========================================================================

fn register_drawing(lua: &Lua, bosl: &mlua::Table) -> LuaResult<()> {
  register_functions(
    lua,
    bosl,
    "std.scad",
    &["stroke", "dashed_stroke", "arc", "helix"],
  )?;

  Ok(())
}

// ===========================================================================
// beziers.scad  (included via std.scad)
// ===========================================================================

fn register_beziers(lua: &Lua, bosl: &mlua::Table) -> LuaResult<()> {
  register_functions(
    lua,
    bosl,
    "std.scad",
    &[
      // Bezier curves
      "bezier_points",
      "bezier_curve",
      "bezier_derivative",
      "bezier_tangent",
      "bezier_curvature",
      "bezier_closest_point",
      "bezier_length",
      "bezier_line_intersection",
      // Bezier path functions
      "bezpath_points",
      "bezpath_curve",
      "bezpath_closest_point",
      "bezpath_length",
      "path_to_bezpath",
      "bezpath_close_to_axis",
      "bezpath_offset",
      // Cubic bezier path construction
      "bez_begin",
      "bez_tang",
      "bez_joint",
      "bez_end",
      // Bezier surfaces
      "is_bezier_patch",
      "bezier_patch_flat",
      "bezier_patch_reverse",
      "bezier_patch_points",
      "bezier_vnf",
      "bezier_vnf_degenerate_patch",
      "bezier_patch_normals",
      "bezier_sheet",
      "bezier_sweep",
      "bezpath_sweep",
      // Debugging
      "debug_bezier",
      "debug_bezier_patches",
    ],
  )?;

  Ok(())
}

// ===========================================================================
// rounding.scad  (included via std.scad)
// ===========================================================================

fn register_rounding(lua: &Lua, bosl: &mlua::Table) -> LuaResult<()> {
  register_functions(
    lua,
    bosl,
    "std.scad",
    &[
      "round_corners",
      "smooth_path",
      "path_join",
      "offset_stroke",
      "offset_sweep",
      "convex_offset_extrude",
      "rounded_prism",
    ],
  )?;

  Ok(())
}

// ===========================================================================
// skin.scad  (included via std.scad)
// ===========================================================================

fn register_skin(lua: &Lua, bosl: &mlua::Table) -> LuaResult<()> {
  register_functions(
    lua,
    bosl,
    "std.scad",
    &[
      "skin",
      "linear_sweep",
      "spiral_sweep",
      "path_sweep",
      "path_sweep2d",
      "sweep",
    ],
  )?;

  Ok(())
}

// ===========================================================================
// vnf.scad  (included via std.scad)
// ===========================================================================

fn register_vnf(lua: &Lua, bosl: &mlua::Table) -> LuaResult<()> {
  register_functions(
    lua,
    bosl,
    "std.scad",
    &[
      "vnf_vertex_array",
      "vnf_tri_array",
      "vnf_join",
      "vnf_from_polygons",
      "vnf_from_region",
      "vnf_merge_points",
      "vnf_drop_unused_points",
      "vnf_triangulate",
      "vnf_slice",
      "vnf_bend",
      "vnf_reverse_faces",
      "vnf_quantize",
      "vnf_polyhedron",
      "vnf_wireframe",
      "debug_vnf",
    ],
  )?;

  Ok(())
}

// ===========================================================================
// threading.scad  (NOT in std.scad — needs separate include)
// ===========================================================================

fn register_threading(lua: &Lua, bosl: &mlua::Table) -> LuaResult<()> {
  register_functions(
    lua,
    bosl,
    "threading.scad",
    &[
      "threaded_rod",
      "threaded_nut",
      "trapezoidal_threaded_rod",
      "trapezoidal_threaded_nut",
      "acme_threaded_rod",
      "acme_threaded_nut",
      "npt_threaded_rod",
      "bspp_threaded_rod",
      "buttress_threaded_rod",
      "buttress_threaded_nut",
      "square_threaded_rod",
      "square_threaded_nut",
      "ball_screw_rod",
      "generic_threaded_rod",
      "generic_threaded_nut",
      "thread_helix",
    ],
  )?;

  Ok(())
}

// ===========================================================================
// screws.scad  (NOT in std.scad)
// ===========================================================================

fn register_screws(lua: &Lua, bosl: &mlua::Table) -> LuaResult<()> {
  register_functions(
    lua,
    bosl,
    "screws.scad",
    &[
      "screw",
      "screw_hole",
      "shoulder_screw",
      "screw_head",
      "nut",
      "nut_trap_side",
      "nut_trap_inline",
      "screw_info",
      "nut_info",
      "thread_specification",
    ],
  )?;

  Ok(())
}

// ===========================================================================
// screw_drive.scad  (NOT in std.scad)
// ===========================================================================

fn register_screw_drive(lua: &Lua, bosl: &mlua::Table) -> LuaResult<()> {
  register_functions(
    lua,
    bosl,
    "screw_drive.scad",
    &[
      "phillips_mask",
      "hex_drive_mask",
      "torx_mask",
      "torx_mask2d",
      "robertson_mask",
      "phillips_depth",
      "phillips_diam",
      "torx_info",
      "torx_diam",
      "torx_depth",
    ],
  )?;

  Ok(())
}

// ===========================================================================
// gears.scad  (NOT in std.scad)
// ===========================================================================

fn register_gears(lua: &Lua, bosl: &mlua::Table) -> LuaResult<()> {
  register_functions(
    lua,
    bosl,
    "gears.scad",
    &[
      // Gear modules
      "spur_gear",
      "spur_gear2d",
      "ring_gear",
      "ring_gear2d",
      "rack",
      "rack2d",
      "crown_gear",
      "bevel_gear",
      "worm",
      "enveloping_worm",
      "worm_gear",
      "planetary_gears",
      // Dimension functions
      "circular_pitch",
      "diametral_pitch",
      "module_value",
      "pitch_radius",
      "outer_radius",
      "root_radius",
      "bevel_pitch_angle",
      "worm_gear_thickness",
      "worm_dist",
      "gear_dist",
      "gear_dist_skew",
      "gear_skew_angle",
      "get_profile_shift",
      "auto_profile_shift",
      "gear_shorten",
      "gear_shorten_skew",
    ],
  )?;

  Ok(())
}

// ===========================================================================
// joiners.scad  (NOT in std.scad)
// ===========================================================================

fn register_joiners(lua: &Lua, bosl: &mlua::Table) -> LuaResult<()> {
  register_functions(
    lua,
    bosl,
    "joiners.scad",
    &[
      "half_joiner_clear",
      "half_joiner",
      "half_joiner2",
      "joiner_clear",
      "joiner",
      "dovetail",
      "snap_pin",
      "snap_pin_socket",
      "rabbit_clip",
      "hirth",
    ],
  )?;

  Ok(())
}

// ===========================================================================
// sliders.scad  (NOT in std.scad)
// ===========================================================================

fn register_sliders(lua: &Lua, bosl: &mlua::Table) -> LuaResult<()> {
  register_functions(lua, bosl, "sliders.scad", &["slider", "rail"])?;

  Ok(())
}

// ===========================================================================
// linear_bearings.scad  (NOT in std.scad)
// ===========================================================================

fn register_linear_bearings(lua: &Lua, bosl: &mlua::Table) -> LuaResult<()> {
  register_functions(
    lua,
    bosl,
    "linear_bearings.scad",
    &["linear_bearing", "lmXuu_info", "linear_bearing_housing"],
  )?;

  Ok(())
}

// ===========================================================================
// nema_steppers.scad  (NOT in std.scad)
// ===========================================================================

fn register_nema_steppers(lua: &Lua, bosl: &mlua::Table) -> LuaResult<()> {
  register_functions(
    lua,
    bosl,
    "nema_steppers.scad",
    &[
      // Functions
      "nema_motor_width",
      "nema_motor_plinth_height",
      "nema_motor_plinth_diam",
      "nema_motor_screw_spacing",
      "nema_motor_screw_size",
      "nema_motor_screw_depth",
      // Motor models
      "nema11_stepper",
      "nema14_stepper",
      "nema17_stepper",
      "nema23_stepper",
      "nema34_stepper",
      // Masking modules
      "nema_mount_holes",
      "nema11_mount_holes",
      "nema14_mount_holes",
      "nema17_mount_holes",
      "nema23_mount_holes",
      "nema34_mount_holes",
    ],
  )?;

  Ok(())
}

// ===========================================================================
// wiring.scad  (NOT in std.scad)
// ===========================================================================

fn register_wiring(lua: &Lua, bosl: &mlua::Table) -> LuaResult<()> {
  register_functions(
    lua,
    bosl,
    "wiring.scad",
    &["hex_offset_ring", "hex_offsets", "wiring"],
  )?;

  Ok(())
}

// ===========================================================================
// walls.scad  (NOT in std.scad)
// ===========================================================================

fn register_walls(lua: &Lua, bosl: &mlua::Table) -> LuaResult<()> {
  register_functions(
    lua,
    bosl,
    "walls.scad",
    &[
      "narrowing_strut",
      "thinning_wall",
      "thinning_triangle",
      "sparse_strut",
      "sparse_strut3d",
      "corrugated_wall",
    ],
  )?;

  Ok(())
}

// ===========================================================================
// ball_bearings.scad  (NOT in std.scad)
// ===========================================================================

fn register_ball_bearings(lua: &Lua, bosl: &mlua::Table) -> LuaResult<()> {
  register_functions(
    lua,
    bosl,
    "ball_bearings.scad",
    &["ball_bearing", "ball_bearing_info"],
  )?;

  Ok(())
}

// ===========================================================================
// bottlecaps.scad  (NOT in std.scad)
// ===========================================================================

fn register_bottlecaps(lua: &Lua, bosl: &mlua::Table) -> LuaResult<()> {
  register_functions(
    lua,
    bosl,
    "bottlecaps.scad",
    &[
      "pco1810_neck",
      "pco1810_cap",
      "pco1881_neck",
      "pco1881_cap",
      "generic_bottle_neck",
      "generic_bottle_cap",
      "bottle_adapter_neck_to_cap",
      "bottle_adapter_cap_to_cap",
      "bottle_adapter_neck_to_neck",
      "sp_neck",
    ],
  )?;

  Ok(())
}

// ===========================================================================
// cubetruss.scad  (NOT in std.scad)
// ===========================================================================

fn register_cubetruss(lua: &Lua, bosl: &mlua::Table) -> LuaResult<()> {
  register_functions(
    lua,
    bosl,
    "cubetruss.scad",
    &[
      "cubetruss",
      "cubetruss_corner",
      "cubetruss_support",
      "cubetruss_clip",
      "cubetruss_foot",
      "cubetruss_joiner",
      "cubetruss_uclip",
      "cubetruss_segment",
      "cubetruss_dist",
    ],
  )?;

  Ok(())
}

// ===========================================================================
// hinges.scad  (NOT in std.scad)
// ===========================================================================

fn register_hinges(lua: &Lua, bosl: &mlua::Table) -> LuaResult<()> {
  register_functions(
    lua,
    bosl,
    "hinges.scad",
    &["knuckle_hinge", "living_hinge_mask"],
  )?;

  Ok(())
}

// ===========================================================================
// polyhedra.scad  (NOT in std.scad)
// ===========================================================================

fn register_polyhedra(lua: &Lua, bosl: &mlua::Table) -> LuaResult<()> {
  register_functions(lua, bosl, "polyhedra.scad", &["regular_polyhedron"])?;

  Ok(())
}

// ===========================================================================
// tripod_mounts.scad  (NOT in std.scad)
// ===========================================================================

fn register_tripod_mounts(lua: &Lua, bosl: &mlua::Table) -> LuaResult<()> {
  register_functions(
    lua,
    bosl,
    "tripod_mounts.scad",
    &["manfrotto_rc2_plate", "tripod_mount"],
  )?;

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
      if let LuaValue::UserData(ud) = val {
        if let Ok(geom) = ud.borrow::<CsgGeometry>() {
          if let Some(ref scad) = geom.scad {
            nodes.push(scad.clone());
          }
        }
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
      "return bosl.cuboid { {10, 20, 30}, rounding = 2, center = true }",
    );
    assert_eq!(nodes.len(), 1);
    if let ScadNode::BoslCall { args, .. } = &nodes[0] {
      assert!(args.contains("[10, 20, 30]"));
      assert!(args.contains("center = true"));
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
        BoslPreviewParams::Cuboid { w, d, h, center } => {
          assert_eq!(*w, 10.0);
          assert_eq!(*d, 20.0);
          assert_eq!(*h, 30.0);
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
        BoslPreviewParams::Cuboid { w, d, h, center } => {
          assert_eq!(*w, 40.0);
          assert_eq!(*d, 40.0);
          assert_eq!(*h, 40.0);
          assert!(*center);
        }
        other => panic!("Expected Cuboid preview, got {:?}", other),
      }
    } else {
      panic!("Expected BoslCall");
    }
  }

  #[test]
  fn bosl_cuboid_preview_center_false() {
    let nodes =
      run_bosl_lua("return bosl.cuboid { {10, 20, 30}, center = false }");
    if let ScadNode::BoslCall { preview, .. } = &nodes[0] {
      match preview {
        BoslPreviewParams::Cuboid { center, .. } => {
          assert!(!*center);
        }
        other => panic!("Expected Cuboid preview, got {:?}", other),
      }
    } else {
      panic!("Expected BoslCall");
    }
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
        BoslPreviewParams::Cuboid { w, d, h, center } => {
          assert_eq!(*w, 40.0);
          assert_eq!(*d, 40.0);
          assert_eq!(*h, 40.0);
          assert!(*center);
        }
        other => panic!("Expected Cuboid preview, got {:?}", other),
      }
    } else {
      panic!("Expected BoslCall");
    }
  }
}
