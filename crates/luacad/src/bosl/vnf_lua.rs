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

// ---------------------------------------------------------------------------
// Measuring and inspecting a mesh
// ---------------------------------------------------------------------------

fn v_sub(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
  [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn v_cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
  [
    a[1] * b[2] - a[2] * b[1],
    a[2] * b[0] - a[0] * b[2],
    a[0] * b[1] - a[1] * b[0],
  ]
}

fn v_dot(a: [f64; 3], b: [f64; 3]) -> f64 {
  a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn v_norm(a: [f64; 3]) -> f64 {
  v_dot(a, a).sqrt()
}

/// A face's area vector: the direction it faces, scaled by its area.
///
/// Summing the edge cross-products this way works for any planar polygon,
/// convex or not, and does not care where the vertices start.
fn face_area_vector(poly: &[[f64; 3]]) -> [f64; 3] {
  let n = poly.len();
  let mut acc = [0.0; 3];
  for i in 0..n {
    let c = v_cross(poly[i], poly[(i + 1) % n]);
    for k in 0..3 {
      acc[k] += c[k];
    }
  }
  [acc[0] / 2.0, acc[1] / 2.0, acc[2] / 2.0]
}

/// Whether a value looks like a VNF: a pair of a point list and a face list.
fn looks_like_vnf(v: &Val) -> bool {
  let Some(items) = v.as_list() else {
    return false;
  };
  if items.len() != 2 {
    return false;
  }
  let (Some(points), Some(faces)) = (items[0].as_list(), items[1].as_list())
  else {
    return false;
  };
  let points_ok = points.is_empty()
    || (points.len() >= 3
      && points[0].as_vec().map(|p| p.len() == 3).unwrap_or(false));
  let faces_ok = faces.is_empty() || faces[0].as_vec().is_some();
  points_ok && faces_ok
}

fn is_vnf(_lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  Ok(LuaValue::Boolean(
    a.val("x").map(|v| looks_like_vnf(&v)).unwrap_or(false),
  ))
}

fn is_vnf_list(_lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let ok = a
    .val("x")
    .and_then(|v| Some(v.as_list()?.iter().all(looks_like_vnf)))
    .unwrap_or(false);
  Ok(LuaValue::Boolean(ok))
}

fn vnf_vertices(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let vnf = read_vnf(a, "vnf")?;
  Val::list(vnf.points.iter().map(|p| Val::vec(*p))).to_lua(lua)
}

fn vnf_faces(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let vnf = read_vnf(a, "vnf")?;
  Val::list(
    vnf
      .faces
      .iter()
      .map(|f| Val::vec(f.iter().map(|i| *i as f64))),
  )
  .to_lua(lua)
}

/// The volume a closed mesh encloses.
///
/// Each face is fanned into triangles from its first vertex and each triangle
/// spans a tetrahedron back to the origin; the signed volumes cancel
/// everywhere outside the solid. A mesh with holes or inconsistently wound
/// faces gives a meaningless answer rather than an error, as in BOSL2.
fn vnf_volume(_lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let vnf = read_vnf(a, "vnf")?;
  let mut total = 0.0;
  for face in &vnf.faces {
    for j in 1..face.len().saturating_sub(1) {
      let (p0, p1, p2) = (
        vnf.points[face[0]],
        vnf.points[face[j]],
        vnf.points[face[j + 1]],
      );
      total += v_dot(v_cross(p2, p1), p0);
    }
  }
  Ok(LuaValue::Number(total / 6.0))
}

fn vnf_area(_lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let vnf = read_vnf(a, "vnf")?;
  let total: f64 = vnf
    .faces
    .iter()
    .map(|f| {
      let poly: Vec<[f64; 3]> = f.iter().map(|i| vnf.points[*i]).collect();
      v_norm(face_area_vector(&poly))
    })
    .sum();
  Ok(LuaValue::Number(total))
}

/// The box a mesh fits in.
///
/// `fast` measures every point in the list; the careful reading measures only
/// the points some face actually uses, so a stray vertex left behind by an
/// edit does not enlarge the answer.
fn vnf_bounds(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let vnf = read_vnf(a, "vnf")?;
  let fast = a.bool_or("fast", false);
  let used: Vec<[f64; 3]> = if fast {
    vnf.points.clone()
  } else {
    vnf
      .faces
      .iter()
      .flatten()
      .filter_map(|i| vnf.points.get(*i).copied())
      .collect()
  };
  if used.is_empty() {
    return Ok(LuaValue::Nil);
  }
  let mut lo = [f64::INFINITY; 3];
  let mut hi = [f64::NEG_INFINITY; 3];
  for p in &used {
    for k in 0..3 {
      lo[k] = lo[k].min(p[k]);
      hi[k] = hi[k].max(p[k]);
    }
  }
  Val::list([Val::vec(lo), Val::vec(hi)]).to_lua(lua)
}

/// Join the edges that only one face uses into loops.
///
/// On a closed mesh every edge is shared by two faces, so nothing comes back.
/// On an open one the leftovers trace its rim.
fn boundary_loops(vnf: &Vnf) -> Vec<Vec<usize>> {
  use std::collections::HashMap;
  let mut counts: HashMap<(usize, usize), i32> = HashMap::new();
  let mut directed: Vec<(usize, usize)> = Vec::new();
  for face in &vnf.faces {
    for i in 0..face.len() {
      let (u, v) = (face[i], face[(i + 1) % face.len()]);
      *counts.entry((u.min(v), u.max(v))).or_insert(0) += 1;
      directed.push((u, v));
    }
  }
  let mut open: Vec<(usize, usize)> = directed
    .into_iter()
    .filter(|(u, v)| counts[&((*u).min(*v), (*u).max(*v))] == 1)
    .collect();

  let mut loops: Vec<Vec<usize>> = Vec::new();
  while let Some(start) = open.pop() {
    let mut path = vec![start.0, start.1];
    loop {
      let tail = *path.last().unwrap();
      match open.iter().position(|(u, _)| *u == tail) {
        Some(i) => {
          let (_, v) = open.remove(i);
          if v == path[0] {
            break;
          }
          path.push(v);
        }
        None => break,
      }
    }
    if path.len() >= 3 {
      loops.push(path);
    }
  }
  loops
}

fn vnf_boundary(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let vnf = read_vnf(a, "vnf")?;
  let merge = a.bool_or("merge", true);
  let idx = a.bool_or("idx", false);
  if idx && merge {
    return a.err("indices can only be returned when merge is false");
  }
  let vnf = if merge { vnf.merged(EPS) } else { vnf };
  let loops = boundary_loops(&vnf);
  Val::list(loops.iter().map(|path| {
    if idx {
      Val::vec(path.iter().map(|i| *i as f64))
    } else {
      Val::list(path.iter().map(|i| Val::vec(vnf.points[*i])))
    }
  }))
  .to_lua(lua)
}

fn vnf_hull(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  // A bare 3D point list is accepted as well as a whole mesh.
  let points: Vec<[f64; 3]> = match read_vnf(a, "vnf") {
    // Only the points some face actually uses, so a vertex left behind by an
    // edit does not stretch the hull.
    Ok(vnf) => {
      let mut used: Vec<usize> = vnf.faces.iter().flatten().copied().collect();
      used.sort_unstable();
      used.dedup();
      used
        .iter()
        .filter_map(|i| vnf.points.get(*i).copied())
        .collect()
    }
    Err(_) => match a.points3("vnf") {
      Some(p) => p,
      None => return a.err("vnf must be a VNF or a list of 3D points"),
    },
  };
  let Some(tris) = crate::bosl::geom::hull3d(&points) else {
    return a.err("the points are all in one plane, so they enclose nothing");
  };
  write_vnf(
    lua,
    &Vnf {
      points,
      faces: tris.iter().map(|t| t.to_vec()).collect(),
    },
  )
}

/// Move every vertex out along the average of the normals meeting there.
///
/// Each face's normal is weighted by how much of the corner it takes up, so a
/// vertex where a broad face meets a narrow one follows the broad one. Only
/// good for offsets small enough not to fold the surface over.
fn offset_points(vnf: &Vnf, delta: f64) -> Vec<[f64; 3]> {
  let normals: Vec<[f64; 3]> = vnf
    .faces
    .iter()
    .map(|f| {
      let poly: Vec<[f64; 3]> = f.iter().map(|i| vnf.points[*i]).collect();
      let n = face_area_vector(&poly);
      let len = v_norm(n);
      if len < EPS {
        [0.0; 3]
      } else {
        [n[0] / len, n[1] / len, n[2] / len]
      }
    })
    .collect();

  let mut acc = vec![[0.0f64; 3]; vnf.points.len()];
  for (fi, face) in vnf.faces.iter().enumerate() {
    let m = face.len();
    for (k, vi) in face.iter().enumerate() {
      let prev = vnf.points[face[(k + m - 1) % m]];
      let here = vnf.points[*vi];
      let next = vnf.points[face[(k + 1) % m]];
      let u = v_sub(prev, here);
      let v = v_sub(next, here);
      let (lu, lv) = (v_norm(u), v_norm(v));
      if lu < EPS || lv < EPS {
        continue;
      }
      let angle = (v_dot(u, v) / (lu * lv)).clamp(-1.0, 1.0).acos();
      for c in 0..3 {
        acc[*vi][c] += normals[fi][c] * angle;
      }
    }
  }
  vnf
    .points
    .iter()
    .enumerate()
    .map(|(i, p)| {
      let len = v_norm(acc[i]);
      if len < EPS {
        *p
      } else {
        [
          p[0] + acc[i][0] / len * delta,
          p[1] + acc[i][1] / len * delta,
          p[2] + acc[i][2] / len * delta,
        ]
      }
    })
    .collect()
}

fn vnf_small_offset(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let vnf = read_vnf(a, "vnf")?;
  let delta = a.need_num("delta")?;
  let merge = a.bool_or("merge", true);
  let vnf = if merge { vnf.merged(EPS) } else { vnf };
  write_vnf(
    lua,
    &Vnf {
      points: offset_points(&vnf, delta),
      faces: vnf.faces.clone(),
    },
  )
}

/// Give an open surface a thickness, so it becomes a solid shell.
///
/// The surface is offset inward by `thickness`, the copy is turned to face
/// the other way, and the two are stitched together around every rim.
fn vnf_sheet(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let vnf = read_vnf(a, "vnf")?;
  let thickness = a.need_num("thickness")?;
  let merge = a.bool_or("merge", true);
  let vnf = if merge { vnf.merged(EPS) } else { vnf };

  let inner = offset_points(&vnf, -thickness);
  let n = vnf.points.len();
  let mut points = vnf.points.clone();
  points.extend(inner);

  let mut faces: Vec<Vec<usize>> = vnf.faces.clone();
  // The offset copy faces the other way, so its winding is reversed.
  for face in &vnf.faces {
    faces.push(face.iter().rev().map(|i| i + n).collect());
  }
  // A wall around each rim joins the two surfaces into one solid. The rim
  // runs the way the outer surface's faces do, so the wall has to be wound
  // against it to face outward.
  for path in boundary_loops(&vnf) {
    for i in 0..path.len() {
      let (u, v) = (path[i], path[(i + 1) % path.len()]);
      faces.push(vec![u, u + n, v + n, v]);
    }
  }
  write_vnf(lua, &Vnf { points, faces })
}

/// Keep only the part of a mesh on one side of a plane.
///
/// The plane is `[a, b, c, d]`, and the side kept is where `ax + by + cz >= d`.
/// Faces straddling it are cut, and unless `closed` is false the opening left
/// behind is covered over.
fn vnf_halfspace(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let Some(plane) = a.nums("plane").filter(|p| p.len() == 4) else {
    return a.err("plane must be [a, b, c, d]");
  };
  let vnf = read_vnf(a, "vnf")?;
  let closed = a.bool_or("closed", true);
  let normal = [plane[0], plane[1], plane[2]];
  let len = v_norm(normal);
  if len < EPS {
    return a.err("the plane's normal must not be zero");
  }
  let side = |p: [f64; 3]| (v_dot(normal, p) - plane[3]) / len;

  let mut points: Vec<[f64; 3]> = Vec::new();
  let mut faces: Vec<Vec<usize>> = Vec::new();
  let index_of = |points: &mut Vec<[f64; 3]>, p: [f64; 3]| -> usize {
    match points.iter().position(|q| v_norm(v_sub(*q, p)) < 1e-7) {
      Some(i) => i,
      None => {
        points.push(p);
        points.len() - 1
      }
    }
  };

  for face in &vnf.faces {
    // Walk the face, keeping what is inside and cutting where it crosses.
    let m = face.len();
    let mut kept: Vec<[f64; 3]> = Vec::new();
    for i in 0..m {
      let p = vnf.points[face[i]];
      let q = vnf.points[face[(i + 1) % m]];
      let (sp, sq) = (side(p), side(q));
      if sp >= -EPS {
        kept.push(p);
      }
      if (sp > EPS && sq < -EPS) || (sp < -EPS && sq > EPS) {
        let t = sp / (sp - sq);
        kept.push([
          p[0] + (q[0] - p[0]) * t,
          p[1] + (q[1] - p[1]) * t,
          p[2] + (q[2] - p[2]) * t,
        ]);
      }
    }
    if kept.len() >= 3 {
      let idx: Vec<usize> =
        kept.iter().map(|p| index_of(&mut points, *p)).collect();
      faces.push(idx);
    }
  }

  let mut result = Vnf { points, faces };
  if closed {
    // Whatever rim the cut left is a hole in the plane; covering it makes
    // the result a solid again. The rim runs the way the cut faces do, so
    // the cover is wound against it.
    for path in boundary_loops(&result) {
      if path.len() >= 3 {
        result.faces.push(path.iter().rev().copied().collect());
      }
    }
  }
  write_vnf(lua, &result)
}

/// Merge faces that lie in the same plane and share an edge.
fn vnf_unify_faces(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let vnf = read_vnf(a, "vnf")?;
  let normal_of = |face: &[usize]| -> Option<[f64; 3]> {
    let poly: Vec<[f64; 3]> = face.iter().map(|i| vnf.points[*i]).collect();
    let n = face_area_vector(&poly);
    let len = v_norm(n);
    (len > EPS).then(|| [n[0] / len, n[1] / len, n[2] / len])
  };

  let mut faces: Vec<Vec<usize>> = vnf.faces.clone();
  let mut merged = true;
  while merged {
    merged = false;
    'outer: for i in 0..faces.len() {
      for j in (i + 1)..faces.len() {
        let (Some(ni), Some(nj)) = (normal_of(&faces[i]), normal_of(&faces[j]))
        else {
          continue;
        };
        if v_dot(ni, nj) < 1.0 - 1e-9 {
          continue;
        }
        if let Some(joined) = join_on_shared_edge(&faces[i], &faces[j]) {
          faces[i] = joined;
          faces.remove(j);
          merged = true;
          break 'outer;
        }
      }
    }
  }
  write_vnf(
    lua,
    &Vnf {
      points: vnf.points.clone(),
      faces,
    },
  )
}

/// Splice two faces together along the edge they share, if they share one.
fn join_on_shared_edge(f1: &[usize], f2: &[usize]) -> Option<Vec<usize>> {
  let n1 = f1.len();
  let n2 = f2.len();
  for i in 0..n1 {
    let (a1, b1) = (f1[i], f1[(i + 1) % n1]);
    for j in 0..n2 {
      let (a2, b2) = (f2[j], f2[(j + 1) % n2]);
      // The shared edge runs the other way round in the second face, which
      // is what makes the two windings agree once spliced.
      if a1 == b2 && b1 == a2 {
        let mut out: Vec<usize> = Vec::with_capacity(n1 + n2 - 2);
        for k in 1..n1 {
          out.push(f1[(i + k) % n1]);
        }
        for k in 1..n2 {
          out.push(f2[(j + k) % n2]);
        }
        out.dedup();
        if out.len() > 2 && out.first() == out.last() {
          out.pop();
        }
        return (out.len() >= 3).then_some(out);
      }
    }
  }
  None
}

/// Everything wrong with a mesh, as a list of `[name, level, colour,
/// message, where]`.
///
/// BOSL2's version is a module that draws the problems in place. LuaCAD has
/// no `echo` to report them through, so the findings come back as data and
/// the caller decides what to do with them.
fn vnf_validate(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  use std::collections::HashMap;
  let vnf = read_vnf(a, "vnf")?;
  let show_warns = a.bool_or("show_warns", true);
  let mut issues: Vec<(&str, &str, &str, &str, Val)> = Vec::new();

  for (fi, face) in vnf.faces.iter().enumerate() {
    let where_ = Val::Num(fi as f64);
    if face.iter().any(|i| *i >= vnf.points.len()) {
      issues.push((
        "BAD_INDEX",
        "ERROR",
        "cyan",
        "Invalid face vertex index.",
        where_,
      ));
      continue;
    }
    let poly: Vec<[f64; 3]> = face.iter().map(|i| vnf.points[*i]).collect();
    if v_norm(face_area_vector(&poly)) < EPS {
      issues.push((
        "NULL_FACE",
        "WARNING",
        "blue",
        "Face has zero area.",
        where_.clone(),
      ));
    } else if !is_planar(&poly) {
      issues.push((
        "NONPLANAR",
        "ERROR",
        "yellow",
        "Face vertices are not coplanar",
        where_.clone(),
      ));
    }
    if show_warns && face.len() > 3 {
      issues.push((
        "BIG_FACE",
        "WARNING",
        "cyan",
        "Face has more than 3 vertices, and may confuse CGAL",
        where_,
      ));
    }
  }

  // Every edge of a closed, consistently wound mesh is used once each way.
  let mut edges: HashMap<(usize, usize), (i32, i32)> = HashMap::new();
  for face in &vnf.faces {
    for i in 0..face.len() {
      let (u, v) = (face[i], face[(i + 1) % face.len()]);
      let key = (u.min(v), u.max(v));
      let slot = edges.entry(key).or_insert((0, 0));
      if u < v { slot.0 += 1 } else { slot.1 += 1 }
    }
  }
  for ((u, v), (fwd, rev)) in &edges {
    let at = Val::vec([*u as f64, *v as f64]);
    match fwd + rev {
      1 => issues.push(("HOLE_EDGE", "ERROR", "red", "Edge bounds Hole", at)),
      2 if *fwd != 1 || *rev != 1 => issues.push((
        "REVERSAL",
        "ERROR",
        "violet",
        "Faces Reverse Across Edge",
        at,
      )),
      n if n > 2 => issues.push((
        "MULTCONN",
        "ERROR",
        "orange",
        "Multiply Connected Geometry. Too many faces attached at Edge",
        at,
      )),
      _ => {}
    }
  }

  // The same face listed twice, however it is rotated.
  let mut seen: Vec<Vec<usize>> = Vec::new();
  for face in &vnf.faces {
    let mut key = face.clone();
    key.sort_unstable();
    if seen.contains(&key) {
      issues.push((
        "DUP_FACE",
        "ERROR",
        "brown",
        "Multiple instances of the same face.",
        Val::vec(face.iter().map(|i| *i as f64)),
      ));
    } else {
      seen.push(key);
    }
  }

  let out = lua.create_table()?;
  for (i, (name, level, colour, msg, at)) in issues.into_iter().enumerate() {
    let entry = lua.create_table()?;
    entry.set(1, name)?;
    entry.set(2, level)?;
    entry.set(3, colour)?;
    entry.set(4, msg)?;
    entry.set(5, at.to_lua(lua)?)?;
    // Named as well as numbered, because reading `issue.msg` beats
    // remembering that the message is the fourth element.
    entry.set("name", name)?;
    entry.set("level", level)?;
    entry.set("color", colour)?;
    entry.set("msg", msg)?;
    out.set(i + 1, entry)?;
  }
  Ok(LuaValue::Table(out))
}

fn is_planar(poly: &[[f64; 3]]) -> bool {
  if poly.len() <= 3 {
    return true;
  }
  let n = face_area_vector(poly);
  let len = v_norm(n);
  if len < EPS {
    return true;
  }
  let unit = [n[0] / len, n[1] / len, n[2] / len];
  let d = v_dot(unit, poly[0]);
  poly
    .iter()
    .all(|p| (v_dot(unit, *p) - d).abs() < 1e-6 * len.sqrt().max(1.0))
}

/// The outline a mesh casts on the XY plane.
///
/// `cut` takes the cross-section at z = 0 instead of the shadow of the whole
/// solid.
fn projection(lua: &Lua, a: &Args) -> LuaResult<LuaValue> {
  let vnf = read_vnf(a, "vnf")?;
  let cut = a.bool_or("cut", false);
  let outlines: Vec<Vec<[f64; 2]>> = if cut {
    // The rim left by keeping everything below z = 0.
    let kept = halfspace_below(&vnf);
    boundary_loops(&kept)
      .iter()
      .map(|path| {
        path
          .iter()
          .map(|i| [kept.points[*i][0], kept.points[*i][1]])
          .collect()
      })
      .collect()
  } else {
    vnf
      .faces
      .iter()
      .map(|f| {
        f.iter()
          .map(|i| [vnf.points[*i][0], vnf.points[*i][1]])
          .collect::<Vec<[f64; 2]>>()
      })
      .filter(|p: &Vec<[f64; 2]>| {
        crate::bosl::regions::signed_area(p).abs() > EPS
      })
      .collect()
  };
  // Every face's shadow overlaps its neighbours', so they are unioned into
  // one outline rather than left as a pile.
  let merged = outlines.iter().fold(Vec::new(), |acc, p| {
    crate::export::combine_outlines(
      &acc,
      std::slice::from_ref(p),
      crate::export::AreaOp::Union,
    )
  });
  Val::list(
    merged
      .iter()
      .map(|path| Val::list(path.iter().map(|p| Val::vec(*p)))),
  )
  .to_lua(lua)
}

/// Everything of a mesh at or below z = 0, cut where it crosses.
fn halfspace_below(vnf: &Vnf) -> Vnf {
  let mut points: Vec<[f64; 3]> = Vec::new();
  let mut faces: Vec<Vec<usize>> = Vec::new();
  let index_of = |points: &mut Vec<[f64; 3]>, p: [f64; 3]| -> usize {
    match points.iter().position(|q| v_norm(v_sub(*q, p)) < 1e-7) {
      Some(i) => i,
      None => {
        points.push(p);
        points.len() - 1
      }
    }
  };
  for face in &vnf.faces {
    let m = face.len();
    let mut kept: Vec<[f64; 3]> = Vec::new();
    for i in 0..m {
      let p = vnf.points[face[i]];
      let q = vnf.points[face[(i + 1) % m]];
      if p[2] <= EPS {
        kept.push(p);
      }
      if (p[2] > EPS && q[2] < -EPS) || (p[2] < -EPS && q[2] > EPS) {
        let t = p[2] / (p[2] - q[2]);
        kept.push([p[0] + (q[0] - p[0]) * t, p[1] + (q[1] - p[1]) * t, 0.0]);
      }
    }
    if kept.len() >= 3 {
      let idx: Vec<usize> =
        kept.iter().map(|p| index_of(&mut points, *p)).collect();
      faces.push(idx);
    }
  }
  Vnf { points, faces }
}

pub fn register(lua: &Lua, bosl: &mlua::Table) -> LuaResult<()> {
  register_all(
    lua,
    bosl,
    &[
      ("is_vnf", &["x"], is_vnf as PureFn),
      ("is_vnf_list", &["x"], is_vnf_list),
      ("vnf_vertices", &["vnf"], vnf_vertices),
      ("vnf_faces", &["vnf"], vnf_faces),
      ("vnf_volume", &["vnf"], vnf_volume),
      ("vnf_area", &["vnf"], vnf_area),
      ("vnf_bounds", &["vnf", "fast"], vnf_bounds),
      ("vnf_boundary", &["vnf", "merge", "idx"], vnf_boundary),
      ("vnf_hull", &["vnf", "fast"], vnf_hull),
      (
        "vnf_small_offset",
        &["vnf", "delta", "merge"],
        vnf_small_offset,
      ),
      (
        "vnf_sheet",
        &["vnf", "thickness", "style", "merge"],
        vnf_sheet,
      ),
      (
        "vnf_halfspace",
        &["plane", "vnf", "closed", "boundary"],
        vnf_halfspace,
      ),
      ("vnf_unify_faces", &["vnf"], vnf_unify_faces),
      (
        "vnf_validate",
        &[
          "vnf",
          "size",
          "show_warns",
          "check_isects",
          "opacity",
          "adjacent",
          "label_verts",
          "label_faces",
          "wireframe",
        ],
        vnf_validate,
      ),
      ("projection", &["vnf", "cut", "eps"], projection),
    ],
  )?;
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
