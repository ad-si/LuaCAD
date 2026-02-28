/// BOSL (Belfry OpenSCAD Library) function bindings for LuaCAD.
///
/// All functions are registered under the `bosl` Lua namespace table so they
/// can be called as `bosl.cuboid(...)`, `bosl.cyl(...)`, etc.
///
/// Because BOSL is an OpenSCAD library, these functions produce ScadNode-only
/// geometry (no mesh). The generated SCAD output automatically includes the
/// required `use <BOSL/...>` directives.
use csgrs::mesh::Mesh as CsgMesh;
use csgrs::traits::CSG;
use mlua::{Lua, Result as LuaResult, Value as LuaValue};

use crate::geometry::CsgGeometry;
use crate::scad_export::ScadNode;

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

/// Create a CsgGeometry representing a BOSL function call.
fn bosl_geometry(module: &str, function: &str, args: String) -> CsgGeometry {
  CsgGeometry {
    mesh: Some(CsgMesh::<()>::new()),
    color: None,
    scad: Some(ScadNode::BoslCall {
      module: module.to_string(),
      function: function.to_string(),
      args,
      has_children: false,
      children: vec![],
    }),
  }
}

// ---------------------------------------------------------------------------
// Generic BOSL function factory
// ---------------------------------------------------------------------------

/// Create a Lua closure that wraps a BOSL function call.
///
/// The generated function accepts a Lua table of arguments and converts them
/// to an OpenSCAD call string. Example:
///   `bosl.cuboid { {10, 20, 30}, fillet = 2 }` → `cuboid([10, 20, 30], fillet = 2);`
fn make_bosl_fn(
  lua: &Lua,
  module: &'static str,
  function: &'static str,
) -> LuaResult<mlua::Function> {
  lua.create_function(move |_, args: mlua::MultiValue| {
    let scad_args = if args.is_empty() {
      String::new()
    } else if args.len() == 1 {
      match &args[0] {
        LuaValue::Table(t) => lua_table_to_scad_args(t),
        other => lua_val_to_scad(other),
      }
    } else {
      // Multiple positional args
      args
        .iter()
        .map(lua_val_to_scad)
        .collect::<Vec<_>>()
        .join(", ")
    };
    Ok(bosl_geometry(module, function, scad_args))
  })
}

// ---------------------------------------------------------------------------
// Module-specific registration helpers
// ---------------------------------------------------------------------------

/// Register a batch of simple BOSL functions onto a Lua table.
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

/// Register all BOSL functions under `bosl.*` in the given Lua state.
pub fn register_bosl(lua: &Lua) -> LuaResult<()> {
  let bosl = lua.create_table()?;

  register_constants(lua, &bosl)?;
  register_math(lua, &bosl)?;
  register_shapes(lua, &bosl)?;
  register_transforms(lua, &bosl)?;
  register_masks(lua, &bosl)?;
  register_threading(lua, &bosl)?;
  register_paths(lua, &bosl)?;
  register_beziers(lua, &bosl)?;
  register_involute_gears(lua, &bosl)?;
  register_joiners(lua, &bosl)?;
  register_sliders(lua, &bosl)?;
  register_metric_screws(lua, &bosl)?;
  register_linear_bearings(lua, &bosl)?;
  register_nema_steppers(lua, &bosl)?;
  register_phillips_drive(lua, &bosl)?;
  register_torx_drive(lua, &bosl)?;
  register_wiring(lua, &bosl)?;
  register_quaternions(lua, &bosl)?;
  register_triangulation(lua, &bosl)?;
  register_convex_hull(lua, &bosl)?;
  register_debug(lua, &bosl)?;

  lua.globals().set("bosl", bosl)?;
  Ok(())
}

// ===========================================================================
// constants.scad
// ===========================================================================

fn register_constants(lua: &Lua, bosl: &mlua::Table) -> LuaResult<()> {
  // Printer slop
  bosl.set("PRINTER_SLOP", 0.2)?;

  // Directional vectors
  let mk_vec = |x: f64, y: f64, z: f64| -> LuaResult<mlua::Table> {
    let t = lua.create_table()?;
    t.set(1, x)?;
    t.set(2, y)?;
    t.set(3, z)?;
    Ok(t)
  };

  bosl.set("V_LEFT", mk_vec(-1.0, 0.0, 0.0)?)?;
  bosl.set("V_RIGHT", mk_vec(1.0, 0.0, 0.0)?)?;
  bosl.set("V_FWD", mk_vec(0.0, -1.0, 0.0)?)?;
  bosl.set("V_BACK", mk_vec(0.0, 1.0, 0.0)?)?;
  bosl.set("V_DOWN", mk_vec(0.0, 0.0, -1.0)?)?;
  bosl.set("V_UP", mk_vec(0.0, 0.0, 1.0)?)?;
  bosl.set("V_ALLPOS", mk_vec(1.0, 1.0, 1.0)?)?;
  bosl.set("V_ALLNEG", mk_vec(-1.0, -1.0, -1.0)?)?;
  bosl.set("V_ZERO", mk_vec(0.0, 0.0, 0.0)?)?;

  // Aliases
  bosl.set("V_CENTER", mk_vec(0.0, 0.0, 0.0)?)?;
  bosl.set("V_ABOVE", mk_vec(0.0, 0.0, 1.0)?)?;
  bosl.set("V_BELOW", mk_vec(0.0, 0.0, -1.0)?)?;
  bosl.set("V_BEFORE", mk_vec(0.0, -1.0, 0.0)?)?;
  bosl.set("V_BEHIND", mk_vec(0.0, 1.0, 0.0)?)?;
  bosl.set("V_TOP", mk_vec(0.0, 0.0, 1.0)?)?;
  bosl.set("V_BOTTOM", mk_vec(0.0, 0.0, -1.0)?)?;
  bosl.set("V_FRONT", mk_vec(0.0, -1.0, 0.0)?)?;
  bosl.set("V_REAR", mk_vec(0.0, 1.0, 0.0)?)?;

  // Alignments
  bosl.set("ALIGN_POS", 1)?;
  bosl.set("ALIGN_CENTER", 0)?;
  bosl.set("ALIGN_NEG", -1)?;

  // Standard orientations
  bosl.set("ORIENT_X", mk_vec(1.0, 0.0, 0.0)?)?;
  bosl.set("ORIENT_Y", mk_vec(0.0, 1.0, 0.0)?)?;
  bosl.set("ORIENT_Z", mk_vec(0.0, 0.0, 1.0)?)?;
  bosl.set("ORIENT_XNEG", mk_vec(-1.0, 0.0, 0.0)?)?;
  bosl.set("ORIENT_YNEG", mk_vec(0.0, -1.0, 0.0)?)?;
  bosl.set("ORIENT_ZNEG", mk_vec(0.0, 0.0, -1.0)?)?;

  // corner_edge_count function
  register_functions(lua, bosl, "constants.scad", &["corner_edge_count"])?;

  Ok(())
}

// ===========================================================================
// math.scad
// ===========================================================================

fn register_math(lua: &Lua, bosl: &mlua::Table) -> LuaResult<()> {
  // Constants
  bosl.set("PHI", (1.0_f64 + 5.0_f64.sqrt()) / 2.0)?;
  bosl.set("EPSILON", 1e-9_f64)?;

  register_functions(
    lua,
    bosl,
    "math.scad",
    &[
      // Simple calculations
      "quant",
      "quantdn",
      "quantup",
      "constrain",
      "min_index",
      "max_index",
      "posmod",
      "modrange",
      "gaussian_rand",
      "log_rand",
      "segs",
      "lerp",
      "hypot",
      "sinh",
      "cosh",
      "tanh",
      "asinh",
      "acosh",
      "atanh",
      "sum",
      "sum_of_squares",
      "sum_of_sines",
      "mean",
      // Comparisons and logic
      "compare_vals",
      "compare_lists",
      "any",
      "all",
      "count_true",
      // List/array operations
      "replist",
      "in_list",
      "slice",
      "select",
      "reverse",
      "array_subindex",
      "list_range",
      "array_shortest",
      "array_longest",
      "array_pad",
      "array_trim",
      "array_fit",
      "enumerate",
      "array_zip",
      "array_group",
      "flatten",
      "sort",
      "sortidx",
      "unique",
      "list_remove",
      "array_dim",
      // Vector manipulation
      "vmul",
      "vdiv",
      "vabs",
      "normalize",
      "vector_angle",
      "vector_axis",
      // Coordinates manipulation
      "point2d",
      "path2d",
      "point3d",
      "path3d",
      "translate_points",
      "scale_points",
      "rotate_points2d",
      "rotate_points3d",
      // Coordinate systems
      "polar_to_xy",
      "xy_to_polar",
      "xyz_to_planar",
      "planar_to_xyz",
      "cylindrical_to_xyz",
      "xyz_to_cylindrical",
      "spherical_to_xyz",
      "xyz_to_spherical",
      "altaz_to_xyz",
      "xyz_to_altaz",
      // Matrix manipulation
      "ident",
      "matrix_transpose",
      "mat3_to_mat4",
      "matrix3_translate",
      "matrix4_translate",
      "matrix3_scale",
      "matrix4_scale",
      "matrix3_zrot",
      "matrix4_xrot",
      "matrix4_yrot",
      "matrix4_zrot",
      "matrix4_rot_by_axis",
      "matrix3_skew",
      "matrix4_skew_xy",
      "matrix4_skew_xz",
      "matrix4_skew_yz",
      "matrix3_mult",
      "matrix4_mult",
      "matrix3_apply",
      "matrix4_apply",
      // Geometry
      "point_on_segment",
      "point_left_of_segment",
      "point_in_polygon",
      "pointlist_bounds",
      "triangle_area2d",
      "right_of_line2d",
      "collinear",
      "collinear_indexed",
      "plane3pt",
      "plane3pt_indexed",
      "distance_from_plane",
      "coplanar",
      "in_front_of_plane",
      "simplify_path",
      "simplify_path_indexed",
    ],
  )?;

  Ok(())
}

// ===========================================================================
// shapes.scad
// ===========================================================================

fn register_shapes(lua: &Lua, bosl: &mlua::Table) -> LuaResult<()> {
  register_functions(
    lua,
    bosl,
    "shapes.scad",
    &[
      // Cuboids
      "cuboid",
      "span_cube",
      "leftcube",
      "rightcube",
      "fwdcube",
      "backcube",
      "downcube",
      "upcube",
      // Prismoids
      "prismoid",
      "rounded_prismoid",
      "right_triangle",
      // Cylindroids
      "cyl",
      "downcyl",
      "xcyl",
      "ycyl",
      "zcyl",
      "tube",
      "torus",
      // Spheroids
      "staggered_sphere",
      // 3D printing shapes
      "teardrop2d",
      "teardrop",
      "onion",
      "narrowing_strut",
      "thinning_wall",
      "braced_thinning_wall",
      "thinning_triangle",
      "sparse_strut",
      "sparse_strut3d",
      "corrugated_wall",
      // Miscellaneous
      "nil",
      "noop",
      "pie_slice",
      "interior_fillet",
      "slot",
      "arced_slot",
    ],
  )?;

  Ok(())
}

// ===========================================================================
// transforms.scad
// ===========================================================================

fn register_transforms(lua: &Lua, bosl: &mlua::Table) -> LuaResult<()> {
  register_functions(
    lua,
    bosl,
    "transforms.scad",
    &[
      // Translations
      "move",
      "xmove",
      "ymove",
      "zmove",
      "left",
      "right",
      "fwd",
      "forward",
      "back",
      "down",
      "up",
      // Rotations
      "rot",
      "xrot",
      "yrot",
      "zrot",
      // Scaling and mirroring
      "xscale",
      "yscale",
      "zscale",
      "xflip",
      "yflip",
      "zflip",
      // Skewing
      "skew_xy",
      "skew_z",
      "skew_yz",
      "skew_x",
      "skew_xz",
      "skew_y",
      // Translational distributors
      "place_copies",
      "spread",
      "xspread",
      "yspread",
      "zspread",
      "distribute",
      "xdistribute",
      "ydistribute",
      "zdistribute",
      "grid2d",
      "grid3d",
      // Rotational distributors
      "rot_copies",
      "xrot_copies",
      "yrot_copies",
      "zrot_copies",
      "xring",
      "yring",
      "zring",
      "arc_of",
      "ovoid_spread",
      // Reflectional distributors
      "mirror_copy",
      "xflip_copy",
      "yflip_copy",
      "zflip_copy",
      // Mutators
      "half_of",
      "top_half",
      "bottom_half",
      "left_half",
      "right_half",
      "front_half",
      "back_half",
      "chain_hull",
      "extrude_arc",
      // 2D mutators
      "round2d",
      "shell2d",
      // Miscellaneous
      "orient_and_align",
    ],
  )?;

  Ok(())
}

// ===========================================================================
// masks.scad
// ===========================================================================

fn register_masks(lua: &Lua, bosl: &mlua::Table) -> LuaResult<()> {
  register_functions(
    lua,
    bosl,
    "masks.scad",
    &[
      // General masks
      "angle_pie_mask",
      "cylinder_mask",
      // Chamfers
      "chamfer_mask",
      "chamfer_mask_x",
      "chamfer_mask_y",
      "chamfer_mask_z",
      "chamfer",
      "chamfer_cylinder_mask",
      "chamfer_hole_mask",
      // Filleting/rounding
      "fillet_mask",
      "fillet_mask_x",
      "fillet_mask_y",
      "fillet_mask_z",
      "fillet",
      "fillet_angled_edge_mask",
      "fillet_angled_corner_mask",
      "fillet_corner_mask",
      "fillet_cylinder_mask",
      "fillet_hole_mask",
    ],
  )?;

  Ok(())
}

// ===========================================================================
// threading.scad
// ===========================================================================

fn register_threading(lua: &Lua, bosl: &mlua::Table) -> LuaResult<()> {
  register_functions(
    lua,
    bosl,
    "threading.scad",
    &[
      // Generic trapezoidal threading
      "trapezoidal_threaded_rod",
      "trapezoidal_threaded_nut",
      // Triangular threading
      "threaded_rod",
      "threaded_nut",
      // Buttress threading
      "buttress_threaded_rod",
      "buttress_threaded_nut",
      // Metric trapezoidal threading
      "metric_trapezoidal_threaded_rod",
      "metric_trapezoidal_threaded_nut",
      // ACME trapezoidal threading
      "acme_threaded_rod",
      "acme_threaded_nut",
      // Square threading
      "square_threaded_rod",
      "square_threaded_nut",
    ],
  )?;

  Ok(())
}

// ===========================================================================
// paths.scad
// ===========================================================================

fn register_paths(lua: &Lua, bosl: &mlua::Table) -> LuaResult<()> {
  register_functions(
    lua,
    bosl,
    "paths.scad",
    &[
      // Functions
      "simplify2d_path",
      "simplify3d_path",
      "path_length",
      "path2d_regular_ngon",
      "path3d_spiral",
      "points_along_path3d",
      // 2D modules
      "modulated_circle",
      // 3D modules
      "extrude_from_to",
      "extrude_2d_hollow",
      "extrude_2dpath_along_spiral",
      "extrude_2dpath_along_3dpath",
      "extrude_2d_shapes_along_3dpath",
      "trace_polyline",
      "debug_polygon",
    ],
  )?;

  Ok(())
}

// ===========================================================================
// beziers.scad
// ===========================================================================

fn register_beziers(lua: &Lua, bosl: &mlua::Table) -> LuaResult<()> {
  register_functions(
    lua,
    bosl,
    "beziers.scad",
    &[
      // Segment functions
      "bez_point",
      "bezier_segment_closest_point",
      "bezier_segment_length",
      "fillet3pts",
      // Path functions
      "bezier_path_point",
      "bezier_path_closest_point",
      "bezier_path_length",
      "bezier_polyline",
      "fillet_path",
      "bezier_close_to_axis",
      "bezier_offset",
      // Path modules
      "bezier_polygon",
      "linear_extrude_bezier",
      "revolve_bezier",
      "rotate_extrude_bezier",
      "revolve_bezier_solid_to_axis",
      "revolve_bezier_offset_shell",
      "extrude_2d_shapes_along_bezier",
      "extrude_bezier_along_bezier",
      "trace_bezier",
      // Patch functions
      "bezier_patch_point",
      "bezier_triangle_point",
      "bezier_patch",
      "bezier_triangle",
      "bezier_patch_flat",
      "patch_reverse",
      "patch_translate",
      "patch_scale",
      "patch_rotate",
      "patches_translate",
      "patches_scale",
      "patches_rotate",
      "bezier_surface",
      // Bezier surface modules
      "bezier_polyhedron",
      "trace_bezier_patches",
    ],
  )?;

  Ok(())
}

// ===========================================================================
// involute_gears.scad
// ===========================================================================

fn register_involute_gears(lua: &Lua, bosl: &mlua::Table) -> LuaResult<()> {
  register_functions(
    lua,
    bosl,
    "involute_gears.scad",
    &[
      // Functions
      "circular_pitch",
      "diametral_pitch",
      "module_value",
      "adendum",
      "dedendum",
      "pitch_radius",
      "outer_radius",
      "root_radius",
      "base_radius",
      // Modules
      "gear_tooth_profile",
      "gear2d",
      "gear",
      "rack",
    ],
  )?;

  Ok(())
}

// ===========================================================================
// joiners.scad
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
      "joiner_pair_clear",
      "joiner_pair",
      "joiner_quad_clear",
      "joiner_quad",
    ],
  )?;

  Ok(())
}

// ===========================================================================
// sliders.scad
// ===========================================================================

fn register_sliders(lua: &Lua, bosl: &mlua::Table) -> LuaResult<()> {
  register_functions(lua, bosl, "sliders.scad", &["slider", "rail"])?;

  Ok(())
}

// ===========================================================================
// metric_screws.scad
// ===========================================================================

fn register_metric_screws(lua: &Lua, bosl: &mlua::Table) -> LuaResult<()> {
  register_functions(
    lua,
    bosl,
    "metric_screws.scad",
    &[
      // Functions
      "get_metric_bolt_head_size",
      "get_metric_bolt_head_height",
      "get_metric_socket_cap_diam",
      "get_metric_socket_cap_height",
      "get_metric_socket_cap_socket_size",
      "get_metric_socket_cap_socket_depth",
      "get_metric_iso_coarse_thread_pitch",
      "get_metric_iso_fine_thread_pitch",
      "get_metric_iso_superfine_thread_pitch",
      "get_metric_jis_thread_pitch",
      "get_metric_nut_size",
      "get_metric_nut_thickness",
      // Modules
      "screw",
      "metric_bolt",
      "metric_nut",
    ],
  )?;

  Ok(())
}

// ===========================================================================
// linear_bearings.scad
// ===========================================================================

fn register_linear_bearings(lua: &Lua, bosl: &mlua::Table) -> LuaResult<()> {
  register_functions(
    lua,
    bosl,
    "linear_bearings.scad",
    &[
      "get_lmXuu_bearing_diam",
      "get_lmXuu_bearing_length",
      "linear_bearing_housing",
      "lmXuu_housing",
    ],
  )?;

  Ok(())
}

// ===========================================================================
// nema_steppers.scad
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
// phillips_drive.scad
// ===========================================================================

fn register_phillips_drive(lua: &Lua, bosl: &mlua::Table) -> LuaResult<()> {
  register_functions(lua, bosl, "phillips_drive.scad", &["phillips_drive"])?;

  Ok(())
}

// ===========================================================================
// torx_drive.scad
// ===========================================================================

fn register_torx_drive(lua: &Lua, bosl: &mlua::Table) -> LuaResult<()> {
  register_functions(
    lua,
    bosl,
    "torx_drive.scad",
    &[
      // Functions
      "torx_outer_diam",
      "torx_inner_diam",
      "torx_depth",
      "torx_tip_radius",
      "torx_rounding_radius",
      // Modules
      "torx_drive2d",
      "torx_drive",
    ],
  )?;

  Ok(())
}

// ===========================================================================
// wiring.scad
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
// quaternions.scad
// ===========================================================================

fn register_quaternions(lua: &Lua, bosl: &mlua::Table) -> LuaResult<()> {
  register_functions(
    lua,
    bosl,
    "quaternions.scad",
    &[
      // Creation
      "Quat",
      "QuatX",
      "QuatY",
      "QuatZ",
      "QuatXYZ",
      "Q_Ident",
      // Scalar operations
      "Q_Add_S",
      "Q_Sub_S",
      "Q_Mul_S",
      "Q_Div_S",
      // Quaternion operations
      "Q_Add",
      "Q_Sub",
      "Q_Mul",
      "Q_Dot",
      "Q_Neg",
      "Q_Conj",
      // Analysis
      "Q_Norm",
      "Q_Normalize",
      "Q_Dist",
      "Q_Axis",
      "Q_Angle",
      // Conversion and transformation
      "Q_Matrix3",
      "Q_Matrix4",
      "Q_Slerp",
      "Q_Rot_Vector",
      // Rendering module
      "Qrot",
    ],
  )?;

  Ok(())
}

// ===========================================================================
// triangulation.scad
// ===========================================================================

fn register_triangulation(lua: &Lua, bosl: &mlua::Table) -> LuaResult<()> {
  register_functions(
    lua,
    bosl,
    "triangulation.scad",
    &[
      "face_normal",
      "find_convex_vertex",
      "point_in_ear",
      "normalize_vertex_perimeter",
      "is_only_noncolinear_vertex",
      "triangulate_face",
      "triangulate_faces",
    ],
  )?;

  Ok(())
}

// ===========================================================================
// convex_hull.scad
// ===========================================================================

fn register_convex_hull(lua: &Lua, bosl: &mlua::Table) -> LuaResult<()> {
  register_functions(
    lua,
    bosl,
    "convex_hull.scad",
    &["convex_hull", "convex_hull2d", "convex_hull3d"],
  )?;

  Ok(())
}

// ===========================================================================
// debug.scad
// ===========================================================================

fn register_debug(lua: &Lua, bosl: &mlua::Table) -> LuaResult<()> {
  register_functions(
    lua,
    bosl,
    "debug.scad",
    &["debug_vertices", "debug_faces", "debug_polyhedron"],
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
    register_bosl(&lua).expect("Failed to register BOSL");

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
      assert_eq!(module, "shapes.scad");
      assert_eq!(function, "cuboid");
      assert!(args.contains("[10, 20, 30]"));
    } else {
      panic!("Expected BoslCall, got {:?}", nodes[0]);
    }
  }

  #[test]
  fn bosl_cuboid_with_named_args() {
    let nodes = run_bosl_lua(
      "return bosl.cuboid { {10, 20, 30}, fillet = 2, center = true }",
    );
    assert_eq!(nodes.len(), 1);
    if let ScadNode::BoslCall { args, .. } = &nodes[0] {
      assert!(args.contains("[10, 20, 30]"));
      assert!(args.contains("center = true"));
      assert!(args.contains("fillet = 2"));
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
      scad.contains("include <BOSL/constants.scad>"),
      "Missing BOSL constants include: {}",
      scad
    );
    assert!(
      scad.contains("use <BOSL/shapes.scad>"),
      "Missing BOSL shapes use: {}",
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
    register_bosl(&lua).expect("Failed to register BOSL");
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
    register_bosl(&lua).expect("Failed to register BOSL");
    let val: f64 = lua
      .load("return bosl.V_UP[3]")
      .eval()
      .expect("Failed to get V_UP[3]");
    assert_eq!(val, 1.0);
  }

  #[test]
  fn bosl_gear_function() {
    let nodes = run_bosl_lua(
      "return bosl.gear { mm_per_tooth = 5, number_of_teeth = 20, thickness = 5 }",
    );
    assert_eq!(nodes.len(), 1);
    if let ScadNode::BoslCall {
      module, function, ..
    } = &nodes[0]
    {
      assert_eq!(module, "involute_gears.scad");
      assert_eq!(function, "gear");
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
    assert!(scad.contains("use <BOSL/shapes.scad>"));
    assert!(scad.contains("use <BOSL/threading.scad>"));
    assert!(scad.contains("include <BOSL/constants.scad>"));
  }
}
