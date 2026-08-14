//! BOSL2's `vnf.scad`: building and editing meshes as `[points, faces]`.
//!
//! A VNF is a pair: the list of vertices, and the list of faces indexing into
//! it. Face indices count from zero, matching OpenSCAD, and faces are wound
//! the way `polyhedron()` wants them.

use mlua::{Lua, Result as LuaResult, Value as LuaValue};

use crate::bosl::value::{Args, PureFn, Val, register_all, v3};
use crate::bosl::vnf::{Caps, Vnf};
use crate::geometry::CsgGeometry;
use crate::scad_export::ScadNode;

const EPS: f64 = 1e-9;

/// Read a VNF from Lua.
pub fn read_vnf(a: &Args, name: &str) -> LuaResult<Vnf> {
  let Some(items) = a.val(name).and_then(|v| v.as_list().map(|s| s.to_vec()))
  else {
    return a.err(format!("{name} must be a VNF, as [points, faces]"));
  };
  if items.len() != 2 {
    return a.err(format!("{name} must be a VNF, as [points, faces]"));
  }
  let Some(points) = items[0].as_matrix() else {
    return a.err(format!("{name}'s points must be a list of coordinates"));
  };
  let Some(faces) = items[1].as_matrix() else {
    return a.err(format!("{name}'s faces must be a list of index lists"));
  };
  Ok(Vnf {
    points: points.iter().map(|p| v3(p)).collect(),
    faces: faces
      .iter()
      .map(|f| f.iter().map(|i| *i as usize).collect())
      .collect(),
  })
}

/// Hand back a mesh that the internal builders produced.
///
/// Those wind their faces counter-clockwise seen from outside, while a VNF
/// in BOSL2 is wound the way `polyhedron()` wants — the other way round — so
/// the faces are flipped on the way out.
fn write_generated(lua: &Lua, vnf: &Vnf) -> LuaResult<LuaValue> {
  write_vnf(lua, &vnf.reversed())
}

/// Hand a VNF back to Lua.
pub fn write_vnf(lua: &Lua, vnf: &Vnf) -> LuaResult<LuaValue> {
  Val::list([
    Val::list(vnf.points.iter().map(|p| Val::vec(*p))),
    Val::list(
      vnf
        .faces
        .iter()
        .map(|f| Val::vec(f.iter().map(|i| *i as f64))),
    ),
  ])
  .to_lua(lua)
}

/// Read the grid of points the array builders work from.
fn read_grid(a: &Args) -> LuaResult<Vec<Vec<[f64; 3]>>> {
  let Some(rows) = a
    .val("points")
    .and_then(|v| v.as_list().map(|s| s.to_vec()))
  else {
    return a.err("points must be a list of rows of points");
  };
  let mut out = Vec::with_capacity(rows.len());
  for row in &rows {
    match row.as_matrix() {
      Some(m) => out.push(m.iter().map(|p| v3(p)).collect()),
      None => return a.err("points must be a list of rows of points"),
    }
  }
  Ok(out)
}

fn caps_of(a: &Args) -> Caps {
  let both = a.bool("caps");
  Caps {
    start: a.bool("cap1").or(both).unwrap_or(false),
    end: a.bool("cap2").or(both).unwrap_or(false),
  }
}

fn vnf_vertex_array(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let rows = read_grid(a)?;
  let vnf = Vnf::vertex_array(
    &rows,
    caps_of(a),
    a.bool_or("col_wrap", false),
    a.bool_or("row_wrap", false),
  );
  let vnf = if a.bool_or("reverse", false) {
    vnf.reversed()
  } else {
    vnf
  };
  write_generated(lua, &vnf)
}

/// Build a surface from rows that need not all be the same length.
///
/// Where two neighbouring rows differ in length the shorter one's points are
/// reused, so the extra points fan out from them rather than leaving a gap.
fn vnf_tri_array(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let rows = read_grid(a)?;
  if rows.len() < 2 {
    return write_vnf(lua, &Vnf::new());
  }
  let caps = caps_of(a);
  let col_wrap = a.bool_or("col_wrap", false);
  let row_wrap = a.bool_or("row_wrap", false);

  let mut points: Vec<[f64; 3]> = Vec::new();
  let mut starts: Vec<usize> = Vec::with_capacity(rows.len());
  for row in &rows {
    starts.push(points.len());
    points.extend_from_slice(row);
  }

  let mut faces: Vec<Vec<usize>> = Vec::new();
  let band = |faces: &mut Vec<Vec<usize>>, ra: usize, rb: usize| {
    let (na, nb) = (rows[ra].len(), rows[rb].len());
    if na == 0 || nb == 0 {
      return;
    }
    let steps = na.max(nb) + usize::from(col_wrap);
    let (mut ia, mut ib) = (0usize, 0usize);
    for _ in 0..steps {
      // Advance whichever row is behind, so the strip stays even when the
      // two rows have different point counts.
      let next_a = (ia + 1).min(na - usize::from(!col_wrap));
      let next_b = (ib + 1).min(nb - usize::from(!col_wrap));
      let ta = (ia + 1) as f64 / na as f64;
      let tb = (ib + 1) as f64 / nb as f64;
      if ta <= tb && ia < na - usize::from(!col_wrap) {
        faces.push(vec![
          starts[ra] + ia % na,
          starts[rb] + ib % nb,
          starts[ra] + next_a % na,
        ]);
        ia = next_a;
      } else if ib < nb - usize::from(!col_wrap) {
        faces.push(vec![
          starts[ra] + ia % na,
          starts[rb] + ib % nb,
          starts[rb] + next_b % nb,
        ]);
        ib = next_b;
      } else {
        break;
      }
    }
  };

  for r in 0..rows.len() - 1 {
    band(&mut faces, r, r + 1);
  }
  if row_wrap {
    band(&mut faces, rows.len() - 1, 0);
  }
  if caps.start && !row_wrap && rows[0].len() >= 3 {
    faces.push((0..rows[0].len()).rev().map(|c| starts[0] + c).collect());
  }
  if caps.end && !row_wrap && rows[rows.len() - 1].len() >= 3 {
    let last = rows.len() - 1;
    faces.push((0..rows[last].len()).map(|c| starts[last] + c).collect());
  }

  let vnf = Vnf { points, faces };
  let vnf = if a.bool_or("reverse", false) {
    vnf.reversed()
  } else {
    vnf
  };
  write_generated(lua, &vnf)
}

fn vnf_join(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let Some(items) = a.val("vnfs").and_then(|v| v.as_list().map(|s| s.to_vec()))
  else {
    return a.err("vnfs must be a list of VNFs");
  };
  let mut out = Vnf::new();
  for (i, item) in items.iter().enumerate() {
    let Some(parts) = item.as_list() else {
      return a.err(format!("entry {i} is not a VNF"));
    };
    if parts.len() != 2 {
      return a.err(format!("entry {i} is not a VNF"));
    }
    let (Some(points), Some(faces)) =
      (parts[0].as_matrix(), parts[1].as_matrix())
    else {
      return a.err(format!("entry {i} is not a VNF"));
    };
    out.join(&Vnf {
      points: points.iter().map(|p| v3(p)).collect(),
      faces: faces
        .iter()
        .map(|f| f.iter().map(|k| *k as usize).collect())
        .collect(),
    });
  }
  write_vnf(lua, &out)
}

fn vnf_from_polygons(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let Some(polys) = a
    .val("polygons")
    .and_then(|v| v.as_list().map(|s| s.to_vec()))
  else {
    return a.err("polygons must be a list of polygons");
  };
  let mut vnf = Vnf::new();
  for poly in &polys {
    let Some(pts) = poly.as_matrix() else {
      return a.err("every entry must be a polygon");
    };
    if pts.len() < 3 {
      continue;
    }
    let base = vnf.points.len();
    vnf.points.extend(pts.iter().map(|p| v3(p)));
    vnf.faces.push((0..pts.len()).map(|i| base + i).collect());
  }
  write_vnf(lua, &vnf)
}

fn vnf_from_region(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let Some(outlines) = a
    .val("region")
    .and_then(|v| v.as_list().map(|s| s.to_vec()))
  else {
    return a.err("region must be a list of outlines");
  };
  let mut vnf = Vnf::new();
  for outline in &outlines {
    let Some(pts) = outline.as_matrix() else {
      return a.err("every outline must be a list of points");
    };
    if pts.len() < 3 {
      continue;
    }
    // A flat region becomes a face lying in the XY plane.
    let base = vnf.points.len();
    vnf.points.extend(pts.iter().map(|p| [p[0], p[1], 0.0]));
    vnf.faces.push((0..pts.len()).map(|i| base + i).collect());
  }
  let vnf = if a.bool_or("reverse", false) {
    vnf.reversed()
  } else {
    vnf
  };
  write_vnf(lua, &vnf)
}

fn vnf_merge_points(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let vnf = read_vnf(a, "vnf")?;
  let eps = a.num_or("eps", EPS);
  write_vnf(lua, &vnf.merged(eps.max(1e-12)))
}

fn vnf_drop_unused_points(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let vnf = read_vnf(a, "vnf")?;
  // Number the vertices the faces actually mention, in the order they first
  // appear, and rebuild the point list to match.
  let mut remap = vec![usize::MAX; vnf.points.len()];
  let mut points = Vec::new();
  for face in &vnf.faces {
    for i in face {
      if *i < remap.len() && remap[*i] == usize::MAX {
        remap[*i] = points.len();
        points.push(vnf.points[*i]);
      }
    }
  }
  let faces = vnf
    .faces
    .iter()
    .map(|f| f.iter().filter_map(|i| remap.get(*i).copied()).collect())
    .collect();
  write_vnf(lua, &Vnf { points, faces })
}

fn vnf_triangulate(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let vnf = read_vnf(a, "vnf")?;
  let mut faces: Vec<Vec<usize>> = Vec::new();
  for face in &vnf.faces {
    if face.len() <= 3 {
      faces.push(face.clone());
      continue;
    }
    // Fanning from the first vertex is enough for the convex faces a VNF
    // normally holds, and matches what the polyhedron backend does anyway.
    for i in 1..face.len() - 1 {
      faces.push(vec![face[0], face[i], face[i + 1]]);
    }
  }
  write_vnf(
    lua,
    &Vnf {
      points: vnf.points,
      faces,
    },
  )
}

fn vnf_reverse_faces(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let vnf = read_vnf(a, "vnf")?;
  write_vnf(lua, &vnf.reversed())
}

fn vnf_quantize(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let vnf = read_vnf(a, "vnf")?;
  let q = a.num_or("q", 2f64.powi(-12));
  if q <= 0.0 {
    return a.err("q must be positive");
  }
  // Snapping coordinates to a grid is what makes vertices that should be
  // the same actually compare equal.
  write_vnf(
    lua,
    &Vnf {
      points: vnf
        .points
        .iter()
        .map(|p| {
          [
            (p[0] / q).round() * q,
            (p[1] / q).round() * q,
            (p[2] / q).round() * q,
          ]
        })
        .collect(),
      faces: vnf.faces,
    },
  )
}

/// Cut a mesh with planes perpendicular to an axis.
fn vnf_slice(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let vnf = read_vnf(a, "vnf")?;
  let axis = match a.string("dir").unwrap_or_else(|| "Z".to_string()).as_str() {
    "X" => 0usize,
    "Y" => 1,
    "Z" => 2,
    other => {
      return a
        .err(format!("dir must be \"X\", \"Y\" or \"Z\", not {other:?}"));
    }
  };
  let mut cuts = match a.val("cuts") {
    Some(Val::Num(c)) => vec![c],
    Some(other) => match other.as_vec() {
      Some(v) => v,
      None => return a.err("cuts must be a distance or a list of them"),
    },
    None => return a.err("cuts is required"),
  };
  cuts.sort_by(f64::total_cmp);

  // Split each face wherever a cut plane crosses it, so the mesh gains
  // vertices along the cut without changing shape.
  let mut out = Vnf {
    points: vnf.points.clone(),
    faces: Vec::new(),
  };
  let index_of = |points: &mut Vec<[f64; 3]>, p: [f64; 3]| -> usize {
    if let Some(i) = points.iter().position(|q| {
      (q[0] - p[0]).abs() < 1e-9
        && (q[1] - p[1]).abs() < 1e-9
        && (q[2] - p[2]).abs() < 1e-9
    }) {
      return i;
    }
    points.push(p);
    points.len() - 1
  };

  for face in &vnf.faces {
    let mut poly: Vec<[f64; 3]> = face
      .iter()
      .filter_map(|i| vnf.points.get(*i).copied())
      .collect();
    for cut in &cuts {
      let mut split: Vec<[f64; 3]> = Vec::new();
      for i in 0..poly.len() {
        let p = poly[i];
        let q = poly[(i + 1) % poly.len()];
        split.push(p);
        let (dp, dq) = (p[axis] - cut, q[axis] - cut);
        if (dp > 0.0) != (dq > 0.0) && (dp - dq).abs() > 1e-12 {
          let t = dp / (dp - dq);
          split.push([
            p[0] + (q[0] - p[0]) * t,
            p[1] + (q[1] - p[1]) * t,
            p[2] + (q[2] - p[2]) * t,
          ]);
        }
      }
      poly = split;
    }
    let indices: Vec<usize> =
      poly.iter().map(|p| index_of(&mut out.points, *p)).collect();
    if indices.len() >= 3 {
      out.faces.push(indices);
    }
  }
  write_vnf(lua, &out)
}

/// Wrap a mesh round a cylinder.
fn vnf_bend(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let vnf = read_vnf(a, "vnf")?;
  let axis = match a.string("axis").unwrap_or_else(|| "Z".to_string()).as_str()
  {
    "X" => 0usize,
    "Y" => 1,
    "Z" => 2,
    other => {
      return a
        .err(format!("axis must be \"X\", \"Y\" or \"Z\", not {other:?}"));
    }
  };
  if vnf.points.is_empty() {
    return write_vnf(lua, &vnf);
  }
  let mut lo = [f64::INFINITY; 3];
  let mut hi = [f64::NEG_INFINITY; 3];
  for p in &vnf.points {
    for i in 0..3 {
      lo[i] = lo[i].min(p[i]);
      hi[i] = hi[i].max(p[i]);
    }
  }

  // The axis the mesh wraps around, and the two it bends in.
  let (flat, height) = match axis {
    0 => (1usize, 2usize),
    1 => (0, 2),
    _ => (0, 1),
  };
  let span = hi[flat] - lo[flat];
  let r = a
    .radius("r", "d", None)
    .unwrap_or_else(|| span / std::f64::consts::TAU);
  if r <= 0.0 {
    return a.err("the bend radius must be positive");
  }

  // Distance along the flat axis becomes angle; distance along the height
  // axis becomes radius.
  let points = vnf
    .points
    .iter()
    .map(|p| {
      let ang = (p[flat] - (lo[flat] + hi[flat]) / 2.0) / r;
      let radius = r + p[height];
      let mut q = *p;
      q[flat] = radius * ang.sin();
      q[height] = radius * ang.cos() - r;
      q
    })
    .collect();
  write_vnf(
    lua,
    &Vnf {
      points,
      faces: vnf.faces,
    },
  )
}

/// Turn a VNF into a solid.
fn vnf_polyhedron(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let vnf = read_vnf(a, "vnf")?;
  // The VNF convention here matches `polyhedron()`, so the faces go through
  // unchanged rather than being flipped as the internal builder's are.
  let node = ScadNode::Polyhedron {
    points: vnf
      .points
      .iter()
      .map(|p| [p[0] as f32, p[1] as f32, p[2] as f32])
      .collect(),
    faces: vnf.faces.clone(),
  };
  let scad = crate::bosl::bosl_node_with_children(
    "std.scad",
    "vnf_polyhedron",
    String::new(),
    vec![],
    Some(node),
  );
  Ok(LuaValue::UserData(lua.create_userdata(CsgGeometry {
    name: None,
    mesh: None,
    color: None,
    scad: Some(scad),
  })?))
}

/// Draw a mesh's edges as thin bars, for inspecting it.
fn vnf_wireframe(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let vnf = read_vnf(a, "vnf")?;
  let width = a.num_or("width", 1.0);
  let mut bars: Vec<ScadNode> = Vec::new();
  let mut seen: std::collections::HashSet<(usize, usize)> =
    std::collections::HashSet::new();
  for face in &vnf.faces {
    for i in 0..face.len() {
      let (u, v) = (face[i], face[(i + 1) % face.len()]);
      let key = (u.min(v), u.max(v));
      if !seen.insert(key) {
        continue;
      }
      let (Some(p), Some(q)) = (vnf.points.get(u), vnf.points.get(v)) else {
        continue;
      };
      bars.push(bar_between(*p, *q, width));
    }
  }
  let scad = crate::bosl::bosl_node_with_children(
    "std.scad",
    "vnf_wireframe",
    String::new(),
    vec![],
    Some(ScadNode::Union(bars)),
  );
  Ok(LuaValue::UserData(lua.create_userdata(CsgGeometry {
    name: None,
    mesh: None,
    color: None,
    scad: Some(scad),
  })?))
}

/// A cylinder spanning two points, used for wireframes and strokes.
pub fn bar_between(p: [f64; 3], q: [f64; 3], width: f64) -> ScadNode {
  let d = [q[0] - p[0], q[1] - p[1], q[2] - p[2]];
  let len = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
  if len < 1e-12 {
    return ScadNode::Union(vec![]);
  }
  let m = crate::bosl::vecmath::Mat4::translate(p)
    .mul(&crate::bosl::vecmath::Mat4::rot_from_to([0.0, 0.0, 1.0], d))
    .mul(&crate::bosl::vecmath::Mat4::translate([
      0.0,
      0.0,
      len / 2.0,
    ]));
  crate::bosl::attach::transform(
    ScadNode::Cylinder {
      r1: (width / 2.0) as f32,
      r2: (width / 2.0) as f32,
      h: len as f32,
      segments: 12,
      center: true,
    },
    m,
  )
}

/// Print a summary of a mesh, for working out why one will not render.
fn debug_vnf(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let vnf = read_vnf(a, "vnf")?;
  let degenerate = vnf.faces.iter().filter(|f| f.len() < 3).count();
  let out_of_range = vnf
    .faces
    .iter()
    .flatten()
    .filter(|i| **i >= vnf.points.len())
    .count();
  println!(
    "vnf: {} points, {} faces, {degenerate} degenerate, \
     {out_of_range} out-of-range indices",
    vnf.points.len(),
    vnf.faces.len(),
  );
  vnf_wireframe(lua, a)
}

pub fn register(lua: &Lua, bosl: &mlua::Table) -> LuaResult<()> {
  register_all(
    lua,
    bosl,
    &[
      (
        "vnf_vertex_array",
        &[
          "points",
          "caps",
          "cap1",
          "cap2",
          "col_wrap",
          "row_wrap",
          "reverse",
          "style",
          "triangulate",
        ],
        vnf_vertex_array as PureFn,
      ),
      (
        "vnf_tri_array",
        &[
          "points", "caps", "cap1", "cap2", "col_wrap", "row_wrap", "reverse",
        ],
        vnf_tri_array,
      ),
      ("vnf_join", &["vnfs"], vnf_join),
      (
        "vnf_from_polygons",
        &["polygons", "fast", "eps"],
        vnf_from_polygons,
      ),
      (
        "vnf_from_region",
        &["region", "transform", "reverse", "triangulate"],
        vnf_from_region,
      ),
      ("vnf_merge_points", &["vnf", "eps"], vnf_merge_points),
      ("vnf_drop_unused_points", &["vnf"], vnf_drop_unused_points),
      ("vnf_triangulate", &["vnf"], vnf_triangulate),
      ("vnf_reverse_faces", &["vnf"], vnf_reverse_faces),
      ("vnf_quantize", &["vnf", "q"], vnf_quantize),
      ("vnf_slice", &["vnf", "dir", "cuts"], vnf_slice),
      ("vnf_bend", &["vnf", "r", "d", "axis"], vnf_bend),
      (
        "vnf_polyhedron",
        &[
          "vnf",
          "convexity",
          "cp",
          "anchor",
          "spin",
          "orient",
          "atype",
        ],
        vnf_polyhedron,
      ),
      ("vnf_wireframe", &["vnf", "width"], vnf_wireframe),
      (
        "debug_vnf",
        &["vnf", "faces", "vertices", "size"],
        debug_vnf,
      ),
    ],
  )
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

  #[test]
  fn a_vertex_array_builds_a_grid_of_faces() {
    let counts: Vec<usize> = eval(
      "local v = bosl.vnf_vertex_array({{{0,0,0},{1,0,0},{2,0,0}},
                                        {{0,1,0},{1,1,0},{2,1,0}}})
       return {#v[1], #v[2]}",
    );
    assert_eq!(counts[0], 6);
    // Two cells, two triangles each.
    assert_eq!(counts[1], 4);
  }

  #[test]
  fn wrapping_the_columns_closes_the_surface_into_a_tube() {
    let open: usize = eval(
      "return #bosl.vnf_vertex_array({{{0,0,0},{1,0,0},{1,1,0}},
                                      {{0,0,1},{1,0,1},{1,1,1}}})[2]",
    );
    let wrapped: usize = eval(
      "return #bosl.vnf_vertex_array({points = {{{0,0,0},{1,0,0},{1,1,0}},
                                                {{0,0,1},{1,0,1},{1,1,1}}},
                                      col_wrap = true})[2]",
    );
    assert!(wrapped > open, "{wrapped} vs {open}");
  }

  #[test]
  fn a_capped_wrapped_array_makes_a_closed_solid() {
    let v = volume(
      "local square = {{-5,-5},{5,-5},{5,5},{-5,5}}
       local rows = {}
       for i, z in ipairs({0, 4}) do
         rows[i] = {}
         for j, p in ipairs(square) do rows[i][j] = {p[1], p[2], z} end
       end
       local vnf = bosl.vnf_vertex_array({points = rows, col_wrap = true,
                                          caps = true})
       render(bosl.vnf_polyhedron(vnf))",
    );
    assert!((v - 400.0).abs() < 1e-3, "{v}");
  }

  #[test]
  fn joining_meshes_concatenates_their_points_and_shifts_the_faces() {
    let counts: Vec<usize> = eval(
      "local a = {{{0,0,0},{1,0,0},{0,1,0}}, {{0,1,2}}}
       local v = bosl.vnf_join({a, a})
       return {#v[1], #v[2], v[2][2][1]}",
    );
    assert_eq!(counts[0], 6);
    assert_eq!(counts[1], 2);
    // The second copy's face points at the second block of vertices.
    assert_eq!(counts[2], 3);
  }

  #[test]
  fn polygons_become_one_face_each() {
    let counts: Vec<usize> = eval(
      "local v = bosl.vnf_from_polygons({{{0,0,0},{1,0,0},{0,1,0}},
                                         {{0,0,1},{1,0,1},{0,1,1}}})
       return {#v[1], #v[2]}",
    );
    assert_eq!(counts, vec![6, 2]);
  }

  #[test]
  fn merging_points_welds_the_duplicates() {
    let n: usize = eval(
      "local v = {{{0,0,0},{1,0,0},{0,1,0},{0,0,0}}, {{0,1,2},{3,1,2}}}
       return #bosl.vnf_merge_points(v)[1]",
    );
    assert_eq!(n, 3);
  }

  #[test]
  fn unused_points_are_dropped_and_the_faces_renumbered() {
    let out: Vec<usize> = eval(
      "local v = {{{0,0,0},{9,9,9},{1,0,0},{0,1,0}}, {{0,2,3}}}
       local w = bosl.vnf_drop_unused_points(v)
       return {#w[1], w[2][1][1], w[2][1][2], w[2][1][3]}",
    );
    assert_eq!(out, vec![3, 0, 1, 2]);
  }

  #[test]
  fn triangulating_splits_every_face_into_triangles() {
    let sizes: Vec<usize> = eval(
      "local v = {{{0,0,0},{1,0,0},{1,1,0},{0,1,0}}, {{0,1,2,3}}}
       local w = bosl.vnf_triangulate(v)
       local out = {}
       for i, f in ipairs(w[2]) do out[i] = #f end
       return out",
    );
    assert_eq!(sizes, vec![3, 3]);
  }

  #[test]
  fn reversing_faces_turns_the_solid_inside_out() {
    let f: Vec<usize> = eval(
      "local v = {{{0,0,0},{1,0,0},{0,1,0}}, {{0,1,2}}}
       return bosl.vnf_reverse_faces(v)[2][1]",
    );
    assert_eq!(f, vec![2, 1, 0]);
  }

  #[test]
  fn quantizing_snaps_the_coordinates_to_a_grid() {
    let p: Vec<f64> = eval(
      "local v = {{{0.126,0,0}}, {}}
       return bosl.vnf_quantize(v, 0.25)[1][1]",
    );
    assert_eq!(p[0], 0.25);
  }

  #[test]
  fn slicing_adds_vertices_where_the_plane_crosses() {
    let counts: Vec<usize> = eval(
      "local v = {{{0,0,0},{10,0,0},{10,0,10}}, {{0,1,2}}}
       local w = bosl.vnf_slice(v, 'X', {5})
       return {#w[1], #w[2][1]}",
    );
    assert!(counts[0] > 3, "{counts:?}");
    assert!(counts[1] > 3, "{counts:?}");
  }

  #[test]
  fn bending_wraps_the_mesh_round_a_cylinder() {
    let spread: f64 = eval(
      "local rows = {}
       for i = 0, 1 do
         rows[i+1] = {}
         for j = 0, 20 do rows[i+1][j+1] = {j - 10, i * 2, 0} end
       end
       local flat = bosl.vnf_vertex_array(rows)
       local bent = bosl.vnf_bend(flat, {r = 3, axis = 'Z'})
       local minx, maxx = math.huge, -math.huge
       for _, p in ipairs(bent[1]) do
         minx = math.min(minx, p[1]); maxx = math.max(maxx, p[1])
       end
       return maxx - minx",
    );
    // Wrapping 20 units of length round r=3 more than closes the circle, so
    // the result spans the bent cylinder's diameter rather than its original
    // 20 units of length.
    assert!(spread < 11.0, "{spread}");
  }

  #[test]
  fn a_vnf_becomes_a_solid_that_renders() {
    let v = volume(
      "local pts = {{0,0,0},{10,0,0},{10,10,0},{0,10,0},
                    {0,0,10},{10,0,10},{10,10,10},{0,10,10}}
       local faces = {{0,1,2,3},{4,5,1,0},{7,6,5,4},{5,6,2,1},
                      {6,7,3,2},{7,4,0,3}}
       render(bosl.vnf_polyhedron({pts, faces}))",
    );
    assert!((v - 1000.0).abs() < 1e-3, "{v}");
  }

  #[test]
  fn a_wireframe_draws_a_bar_for_every_edge() {
    let v = volume(
      "render(bosl.vnf_wireframe({{{0,0,0},{10,0,0},{0,10,0}}, {{0,1,2}}}, 1))",
    );
    assert!(v > 0.0, "the wireframe should have some volume");
  }
}
