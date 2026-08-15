//! BOSL2's `metric_screws.scad` and the smaller standards tables.
//!
//! These are lookup tables from a nominal size to a real dimension — how wide
//! an M6 bolt head is, how coarse an M10 thread runs. A size between two rows
//! is interpolated rather than rejected, matching OpenSCAD's `lookup()`, so a
//! non-standard size still gives a sensible answer instead of an error.

use mlua::{Lua, Result as LuaResult, Value as LuaValue};

use crate::bosl::value::{Args, PureFn, register_all};
use crate::scad_export::ScadNode;

/// Read a value off a table of `(size, value)` rows.
///
/// Between two rows the answer is interpolated; outside the ends it holds at
/// the nearest one, which is what `lookup()` does.
fn lookup(table: &[(f64, f64)], size: f64) -> f64 {
  if table.is_empty() {
    return 0.0;
  }
  if size <= table[0].0 {
    return table[0].1;
  }
  if size >= table[table.len() - 1].0 {
    return table[table.len() - 1].1;
  }
  for w in table.windows(2) {
    let ((x0, y0), (x1, y1)) = (w[0], w[1]);
    if size >= x0 && size <= x1 {
      let t = if (x1 - x0).abs() < 1e-12 {
        0.0
      } else {
        (size - x0) / (x1 - x0)
      };
      return y0 + (y1 - y0) * t;
    }
  }
  table[table.len() - 1].1
}

/// Every size table, by the name of the function that reads it.
fn table_for(name: &str) -> &'static [(f64, f64)] {
  match name {
    "get_metric_bolt_head_size" => &[
      (3.0, 5.5),
      (4.0, 7.0),
      (5.0, 8.0),
      (6.0, 10.0),
      (7.0, 11.0),
      (8.0, 13.0),
      (10.0, 17.0),
      (12.0, 19.0),
      (14.0, 22.0),
      (16.0, 24.0),
      (18.0, 27.0),
      (20.0, 30.0),
      (24.0, 36.0),
      (30.0, 46.0),
      (36.0, 55.0),
      (42.0, 65.0),
      (48.0, 75.0),
      (56.0, 85.0),
      (64.0, 95.0),
    ],
    "get_metric_bolt_head_height" => &[
      (1.6, 1.23),
      (2.0, 1.53),
      (2.5, 1.83),
      (3.0, 2.13),
      (4.0, 2.93),
      (5.0, 3.65),
      (6.0, 4.15),
      (8.0, 5.45),
      (10.0, 6.58),
      (12.0, 7.68),
      (14.0, 8.98),
      (16.0, 10.18),
      (20.0, 12.72),
      (24.0, 15.35),
      (30.0, 19.12),
      (36.0, 22.92),
      (42.0, 26.42),
      (48.0, 30.42),
      (56.0, 35.50),
      (64.0, 40.50),
    ],
    "get_metric_socket_cap_diam" => &[
      (1.6, 3.0),
      (2.0, 3.8),
      (2.5, 4.5),
      (3.0, 5.5),
      (4.0, 7.0),
      (5.0, 8.5),
      (6.0, 10.0),
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
      (56.0, 84.0),
      (64.0, 96.0),
    ],
    "get_metric_socket_cap_height" => &[
      (1.6, 1.7),
      (2.0, 2.0),
      (2.5, 2.5),
      (3.0, 3.0),
      (4.0, 4.0),
      (5.0, 5.0),
      (6.0, 6.0),
      (8.0, 8.0),
      (10.0, 10.0),
      (12.0, 12.0),
      (14.0, 14.0),
      (16.0, 16.0),
      (18.0, 18.0),
      (20.0, 20.0),
      (22.0, 22.0),
      (24.0, 24.0),
      (27.0, 27.0),
      (30.0, 30.0),
      (33.0, 33.0),
      (36.0, 36.0),
      (42.0, 42.0),
      (48.0, 48.0),
      (56.0, 56.0),
      (64.0, 64.0),
    ],
    "get_metric_socket_cap_socket_size" => &[
      (1.6, 1.5),
      (2.0, 1.5),
      (2.5, 2.0),
      (3.0, 2.5),
      (4.0, 3.0),
      (5.0, 4.0),
      (6.0, 5.0),
      (8.0, 6.0),
      (10.0, 8.0),
      (12.0, 10.0),
      (14.0, 12.0),
      (16.0, 14.0),
      (18.0, 14.0),
      (20.0, 17.0),
      (22.0, 17.0),
      (24.0, 19.0),
      (27.0, 19.0),
      (30.0, 22.0),
      (33.0, 24.0),
      (36.0, 27.0),
      (42.0, 32.0),
      (48.0, 36.0),
      (56.0, 41.0),
      (64.0, 46.0),
    ],
    "get_metric_socket_cap_socket_depth" => &[
      (1.6, 0.7),
      (2.0, 1.0),
      (2.5, 1.1),
      (3.0, 1.3),
      (4.0, 2.0),
      (5.0, 2.5),
      (6.0, 3.0),
      (8.0, 4.0),
      (10.0, 5.0),
      (12.0, 6.0),
      (14.0, 7.0),
      (16.0, 8.0),
      (18.0, 9.0),
      (20.0, 10.0),
      (22.0, 11.0),
      (24.0, 12.0),
      (27.0, 13.5),
      (30.0, 15.5),
      (33.0, 18.0),
      (36.0, 19.0),
      (42.0, 24.0),
      (48.0, 28.0),
      (56.0, 34.0),
      (64.0, 38.0),
    ],
    "get_metric_iso_coarse_thread_pitch" => &[
      (1.6, 0.35),
      (2.0, 0.40),
      (2.5, 0.45),
      (3.0, 0.50),
      (4.0, 0.70),
      (5.0, 0.80),
      (6.0, 1.00),
      (7.0, 1.00),
      (8.0, 1.25),
      (10.0, 1.50),
      (12.0, 1.75),
      (14.0, 2.00),
      (16.0, 2.00),
      (18.0, 2.50),
      (20.0, 2.50),
      (22.0, 2.50),
      (24.0, 3.00),
      (27.0, 3.00),
      (30.0, 3.50),
      (33.0, 3.50),
      (36.0, 4.00),
      (39.0, 4.00),
      (42.0, 4.50),
      (45.0, 4.50),
      (48.0, 5.00),
      (56.0, 5.50),
      (64.0, 6.00),
    ],
    "get_metric_iso_fine_thread_pitch" => &[
      (1.6, 0.35),
      (2.0, 0.40),
      (2.5, 0.45),
      (3.0, 0.50),
      (4.0, 0.70),
      (5.0, 0.80),
      (6.0, 1.00),
      (7.0, 1.00),
      (8.0, 1.00),
      (10.0, 1.25),
      (12.0, 1.50),
      (14.0, 1.50),
      (16.0, 2.00),
      (18.0, 2.50),
      (20.0, 2.50),
      (22.0, 2.50),
      (24.0, 3.00),
      (27.0, 3.00),
      (30.0, 3.50),
      (33.0, 3.50),
      (36.0, 4.00),
      (39.0, 4.00),
      (42.0, 4.50),
      (45.0, 4.50),
      (48.0, 5.00),
      (56.0, 5.50),
      (64.0, 6.00),
    ],
    "get_metric_iso_superfine_thread_pitch" => &[
      (1.6, 0.35),
      (2.0, 0.40),
      (2.5, 0.45),
      (3.0, 0.50),
      (4.0, 0.70),
      (5.0, 0.80),
      (6.0, 1.00),
      (7.0, 1.00),
      (8.0, 1.00),
      (10.0, 1.00),
      (12.0, 1.25),
      (14.0, 1.50),
      (16.0, 2.00),
      (18.0, 2.50),
      (20.0, 2.50),
      (22.0, 2.50),
      (24.0, 3.00),
      (27.0, 3.00),
      (30.0, 3.50),
      (33.0, 3.50),
      (36.0, 4.00),
      (39.0, 4.00),
      (42.0, 4.50),
      (45.0, 4.50),
      (48.0, 5.00),
      (56.0, 5.50),
      (64.0, 6.00),
    ],
    "get_metric_jis_thread_pitch" => &[
      (2.0, 0.40),
      (2.5, 0.45),
      (3.0, 0.50),
      (4.0, 0.70),
      (5.0, 0.80),
      (6.0, 1.00),
      (7.0, 1.00),
      (8.0, 1.25),
      (10.0, 1.25),
      (12.0, 1.25),
      (14.0, 1.50),
      (16.0, 1.50),
      (18.0, 1.50),
      (20.0, 1.50),
    ],
    "get_metric_nut_size" => &[
      (2.0, 4.0),
      (2.5, 5.0),
      (3.0, 5.5),
      (4.0, 7.0),
      (5.0, 8.0),
      (6.0, 10.0),
      (7.0, 11.0),
      (8.0, 13.0),
      (10.0, 17.0),
      (12.0, 19.0),
      (14.0, 22.0),
      (16.0, 24.0),
      (18.0, 27.0),
      (20.0, 30.0),
    ],
    "get_metric_nut_thickness" => &[
      (1.6, 1.3),
      (2.0, 1.6),
      (2.5, 2.0),
      (3.0, 2.4),
      (4.0, 3.2),
      (5.0, 4.0),
      (6.0, 5.0),
      (7.0, 5.5),
      (8.0, 6.5),
      (10.0, 8.0),
      (12.0, 10.0),
      (14.0, 11.0),
      (16.0, 13.0),
      (18.0, 15.0),
      (20.0, 16.0),
      (24.0, 21.5),
      (30.0, 25.6),
      (36.0, 31.0),
      (42.0, 34.0),
      (48.0, 38.0),
      (56.0, 45.0),
      (64.0, 51.0),
    ],
    _ => &[],
  }
}

/// One table reader, by name.
fn reader(name: &'static str) -> impl Fn(&Lua, &Args) -> LuaResult<LuaValue> {
  move |_lua, a| {
    let size = a.need_num("size")?;
    Ok(LuaValue::Number(lookup(table_for(name), size)))
  }
}

// ---------------------------------------------------------------------------
// The fasteners themselves
// ---------------------------------------------------------------------------

fn prism(r: f64, h: f64, sides: u32, center: bool) -> ScadNode {
  ScadNode::Cylinder {
    r1: r as f32,
    r2: r as f32,
    h: h as f32,
    center,
    segments: sides,
  }
}

fn up(node: ScadNode, z: f64) -> ScadNode {
  ScadNode::Translate {
    x: 0.0,
    y: 0.0,
    z: z as f32,
    child: Box::new(node),
  }
}

/// A screw with a shank and a head, sized outright rather than by standard.
fn generic_screw(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let screwsize = a.num_or("screwsize", 3.0);
  let screwlen = a.num_or("screwlen", 10.0);
  let headsize = a.num_or("headsize", 6.0);
  let headlen = a.num_or("headlen", 3.0);
  if screwsize <= 0.0 || screwlen <= 0.0 {
    return a.err("the screw's size and length must be positive");
  }
  let sides = a.segments(screwsize / 2.0);
  let mut parts = vec![up(
    prism(screwsize / 2.0, screwlen, sides, false),
    -screwlen,
  )];
  if headlen > 0.0 && headsize > 0.0 {
    parts.push(prism(
      headsize / 2.0,
      headlen,
      a.segments(headsize / 2.0),
      false,
    ));
  }
  as_geometry(lua, "generic_screw", a, ScadNode::Union(parts))
}

/// A metric bolt, sized from the ISO tables.
fn metric_bolt(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let size = a.num_or("size", 3.0);
  let l = a.num_or("l", 12.0);
  let headtype = a.string("headtype").unwrap_or_else(|| "socket".to_string());
  if size <= 0.0 || l <= 0.0 {
    return a.err("the bolt's size and length must be positive");
  }
  let shaft = prism(size / 2.0, l, a.segments(size / 2.0), false);
  let head = match headtype.as_str() {
    "none" => None,
    "hex" => {
      // Across the flats is what the tables give, so the circumscribed
      // radius the hexagon is drawn from is a little larger.
      let across = lookup(table_for("get_metric_bolt_head_size"), size);
      let h = lookup(table_for("get_metric_bolt_head_height"), size);
      Some(prism(across / 3f64.sqrt(), h, 6, false))
    }
    "socket" | "pan" | "button" | "round" | "cheese" | "flat" => {
      let d = lookup(table_for("get_metric_socket_cap_diam"), size);
      let h = lookup(table_for("get_metric_socket_cap_height"), size);
      Some(prism(d / 2.0, h, a.segments(d / 2.0), false))
    }
    other => {
      return a.err(format!(
        "unknown head type '{other}'; use \"socket\", \"hex\" or \"none\""
      ));
    }
  };
  let mut parts = vec![up(shaft, -l)];
  if let Some(head) = head {
    parts.push(head);
  }
  as_geometry(lua, "metric_bolt", a, ScadNode::Union(parts))
}

/// A metric nut, sized from the ISO tables.
fn metric_nut(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let size = a.num_or("size", 3.0);
  if size <= 0.0 {
    return a.err("the nut's size must be positive");
  }
  let across = lookup(table_for("get_metric_nut_size"), size);
  let thickness = lookup(table_for("get_metric_nut_thickness"), size);
  let body = prism(across / 3f64.sqrt(), thickness, 6, true);
  let node = if a.bool_or("hole", true) {
    // The bore runs right through, a hair proud at each end so the
    // difference leaves no skin behind.
    ScadNode::Difference(vec![
      body,
      prism(size / 2.0, thickness + 0.2, a.segments(size / 2.0), true),
    ])
  } else {
    body
  };
  as_geometry(lua, "metric_nut", a, node)
}

fn as_geometry(
  lua: &Lua,
  name: &'static str,
  a: &Args,
  node: ScadNode,
) -> LuaResult<LuaValue> {
  let scad = crate::bosl::bosl_node_with_children(
    "metric_screws.scad",
    name,
    a.scad_args().to_string(),
    vec![],
    Some(node),
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
// Other standards tables
// ---------------------------------------------------------------------------

/// The inside or outside radius of a modular hose fitting.
fn modular_hose_radius(_lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let size = a.need_num("size")?;
  let outer = a.bool_or("outer", false);
  // The three sizes are named by their nominal hose bore in inches.
  let table: &[(f64, f64, f64)] =
    &[(0.25, 3.4, 6.4), (0.5, 6.5, 11.4), (0.75, 9.5, 16.4)];
  match table.iter().find(|(s, _, _)| (s - size).abs() < 1e-9) {
    Some((_, inner, out)) => {
      Ok(LuaValue::Number(if outer { *out } else { *inner }))
    }
    None => a.err("size must be 1/4, 1/2 or 3/4"),
  }
}

/// The real thread diameter of an SP-series bottle closure.
fn sp_diameter(_lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let diam = a.need_num("diam")?;
  let sp_type = a.num_or("type", 400.0) as i64;
  let table: &[(i64, &[(i64, f64)])] = &[
    (
      400,
      &[
        (18, 17.68),
        (20, 19.69),
        (22, 21.69),
        (24, 23.67),
        (28, 27.38),
        (30, 28.37),
        (33, 31.83),
        (35, 34.34),
        (38, 37.19),
        (40, 39.75),
        (43, 41.63),
        (45, 43.82),
        (48, 47.12),
        (51, 49.56),
        (53, 52.07),
        (58, 56.06),
        (60, 59.06),
        (63, 62.08),
        (66, 65.07),
        (70, 69.06),
        (75, 73.56),
        (77, 76.66),
        (83, 82.58),
        (89, 88.75),
        (100, 99.57),
        (110, 109.58),
        (120, 119.56),
      ],
    ),
    (
      410,
      &[
        (18, 17.68),
        (20, 19.59),
        (22, 21.69),
        (24, 23.67),
        (28, 27.38),
      ],
    ),
    (
      415,
      &[
        (13, 12.90),
        (15, 14.61),
        (18, 17.68),
        (20, 19.69),
        (22, 21.69),
        (24, 23.67),
        (28, 27.38),
        (33, 31.83),
      ],
    ),
  ];
  let Some((_, rows)) = table.iter().find(|(t, _)| *t == sp_type) else {
    return a.err("type must be 400, 410 or 415");
  };
  match rows.iter().find(|(d, _)| *d == diam.round() as i64) {
    Some((_, v)) => Ok(LuaValue::Number(*v)),
    None => a.err(format!(
      "SP{sp_type} has no {diam} closure; the sizes are {}",
      rows
        .iter()
        .map(|(d, _)| d.to_string())
        .collect::<Vec<_>>()
        .join(", ")
    )),
  }
}

pub fn register(lua: &Lua, bosl: &mlua::Table) -> LuaResult<()> {
  use crate::bosl::value::register_pure;
  for name in [
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
  ] {
    let f = reader(name);
    let func = lua.create_function(move |lua, args: mlua::MultiValue| {
      let parsed = Args::parse_pure(name, &["size"], &args)?;
      f(lua, &parsed)
    })?;
    bosl.set(name, func)?;
  }

  register_pure(
    lua,
    bosl,
    "generic_screw",
    &[
      "screwsize",
      "screwlen",
      "headsize",
      "headlen",
      "pitch",
      "countersunk",
      "details",
      "anchor",
      "spin",
      "orient",
    ],
    generic_screw,
  )?;
  register_pure(
    lua,
    bosl,
    "metric_bolt",
    &[
      "headtype", "size", "l", "shank", "pitch", "details", "coarse", "flange",
      "phillips", "torx", "anchor", "spin", "orient",
    ],
    metric_bolt,
  )?;
  register_pure(
    lua,
    bosl,
    "metric_nut",
    &[
      "size", "hole", "pitch", "details", "flange", "center", "anchor", "spin",
      "orient",
    ],
    metric_nut,
  )?;
  register_all(
    lua,
    bosl,
    &[
      (
        "modular_hose_radius",
        &["size", "outer"],
        modular_hose_radius as PureFn,
      ),
      ("sp_diameter", &["diam", "type"], sp_diameter),
    ],
  )
}

#[cfg(test)]
mod tests {
  use super::*;

  #[test]
  fn a_standard_size_reads_straight_off_the_table() {
    let t = table_for("get_metric_bolt_head_size");
    assert_eq!(lookup(t, 6.0), 10.0);
    assert_eq!(lookup(t, 8.0), 13.0);
    let p = table_for("get_metric_iso_coarse_thread_pitch");
    assert_eq!(lookup(p, 10.0), 1.5);
    assert_eq!(lookup(p, 3.0), 0.5);
  }

  #[test]
  fn a_size_between_two_rows_is_interpolated() {
    let t = table_for("get_metric_bolt_head_size");
    // Halfway between M6 (10) and M7 (11).
    assert!((lookup(t, 6.5) - 10.5).abs() < 1e-12);
  }

  #[test]
  fn a_size_off_either_end_holds_at_the_nearest_row() {
    let t = table_for("get_metric_bolt_head_size");
    assert_eq!(lookup(t, 1.0), 5.5);
    assert_eq!(lookup(t, 200.0), 95.0);
  }

  #[test]
  fn every_table_runs_in_increasing_size_order() {
    for name in [
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
    ] {
      let t = table_for(name);
      assert!(!t.is_empty(), "{name} is empty");
      assert!(
        t.windows(2).all(|w| w[1].0 > w[0].0),
        "{name} is not in increasing order"
      );
    }
  }

  #[test]
  fn a_fine_thread_is_never_coarser_than_the_coarse_one() {
    let coarse = table_for("get_metric_iso_coarse_thread_pitch");
    let fine = table_for("get_metric_iso_fine_thread_pitch");
    for size in [3.0, 6.0, 8.0, 10.0, 12.0, 20.0] {
      assert!(
        lookup(fine, size) <= lookup(coarse, size),
        "at M{size}: fine {} vs coarse {}",
        lookup(fine, size),
        lookup(coarse, size)
      );
    }
  }
}
