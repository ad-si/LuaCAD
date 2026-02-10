use crate::geometry::CsgGeometry;
use csgrs::mesh::Mesh as CsgMesh;
use csgrs::traits::CSG;
use threemf::Mesh as ThreemfMesh;

#[derive(Debug, Clone, Copy)]
pub enum ExportFormat {
  ThreeMF,
  PLY,
  STL,
  OBJ,
}

type VKey = (i64, i64, i64);

/// Quantize a float to an integer key for vertex deduplication.
/// Uses micron precision (0.001mm) which is well beyond 3D printing accuracy.
fn quantize(v: f32) -> i64 {
  (v as f64 * 1000.0).round() as i64
}

fn vkey(x: f32, y: f32, z: f32) -> VKey {
  (quantize(x), quantize(y), quantize(z))
}

/// Check if point P lies on the line segment A→B (all in quantized coords).
/// Returns true if P is strictly between A and B (not equal to either endpoint).
fn point_on_segment(a: VKey, b: VKey, p: VKey) -> bool {
  if p == a || p == b {
    return false;
  }
  // Vector AB and AP
  let (abx, aby, abz) = (b.0 - a.0, b.1 - a.1, b.2 - a.2);
  let (apx, apy, apz) = (p.0 - a.0, p.1 - a.1, p.2 - a.2);
  // Cross product must be zero (collinear)
  let cx = aby * apz - abz * apy;
  let cy = abz * apx - abx * apz;
  let cz = abx * apy - aby * apx;
  if cx != 0 || cy != 0 || cz != 0 {
    return false;
  }
  // Dot product must be positive and less than |AB|^2 (between A and B)
  let dot = apx * abx + apy * aby + apz * abz;
  let len_sq = abx * abx + aby * aby + abz * abz;
  dot > 0 && dot < len_sq
}

/// Resolve T-junctions in a polygon by inserting vertices that lie on its edges.
/// Returns a new vertex list with extra vertices inserted along edges.
fn resolve_t_junctions(
  polygon_vkeys: &[VKey],
  all_vkeys: &std::collections::HashSet<VKey>,
) -> Vec<VKey> {
  let mut result = Vec::new();
  let n = polygon_vkeys.len();
  for i in 0..n {
    let a = polygon_vkeys[i];
    let b = polygon_vkeys[(i + 1) % n];
    result.push(a);
    // Find all vertices that lie on edge A→B
    let mut on_edge: Vec<(i64, VKey)> = Vec::new();
    for &v in all_vkeys {
      if point_on_segment(a, b, v) {
        // Parametric position along AB for sorting
        let ab = (b.0 - a.0, b.1 - a.1, b.2 - a.2);
        let av = (v.0 - a.0, v.1 - a.1, v.2 - a.2);
        // Use the largest component for stable sorting
        let t = if ab.0.abs() >= ab.1.abs() && ab.0.abs() >= ab.2.abs() {
          av.0 * 1000 / ab.0
        } else if ab.1.abs() >= ab.2.abs() {
          av.1 * 1000 / ab.1
        } else {
          av.2 * 1000 / ab.2
        };
        on_edge.push((t, v));
      }
    }
    on_edge.sort_by_key(|&(t, _)| t);
    for (_, v) in on_edge {
      result.push(v);
    }
  }
  result
}

/// Export all geometries to a 3MF file. Uses original CAD coordinates (no GL transform).
/// Deduplicates vertices and resolves T-junctions so the mesh is manifold.
pub fn export_3mf(
  geometries: &[CsgGeometry],
  path: &std::path::Path,
) -> Result<(), String> {
  use std::collections::{HashMap, HashSet};

  // Step 1: Collect all unique vertex positions across all polygons
  let mut all_vkeys: HashSet<VKey> = HashSet::new();
  let mut poly_data: Vec<(Vec<VKey>, Vec<[f32; 3]>)> = Vec::new();

  for geom in geometries {
    if geom.mesh.polygons.is_empty() {
      continue;
    }
    let tri_mesh = geom.mesh.triangulate();

    for polygon in &tri_mesh.polygons {
      let keys: Vec<VKey> = polygon
        .vertices
        .iter()
        .map(|v| vkey(v.pos.x, v.pos.y, v.pos.z))
        .collect();
      let coords: Vec<[f32; 3]> = polygon
        .vertices
        .iter()
        .map(|v| [v.pos.x, v.pos.y, v.pos.z])
        .collect();
      for &k in &keys {
        all_vkeys.insert(k);
      }
      poly_data.push((keys, coords));
    }
  }

  if poly_data.is_empty() {
    return Err("No geometry to export".to_string());
  }

  // Step 2: Resolve T-junctions and build output mesh
  let mut vertices: Vec<threemf::model::mesh::Vertex> = Vec::new();
  let mut triangles = Vec::new();
  let mut vertex_map: HashMap<VKey, usize> = HashMap::new();

  // Map from VKey to actual f32 coordinates (first occurrence wins)
  let mut coord_map: HashMap<VKey, [f32; 3]> = HashMap::new();
  for (keys, coords) in &poly_data {
    for (k, c) in keys.iter().zip(coords.iter()) {
      coord_map.entry(*k).or_insert(*c);
    }
  }

  let get_idx = |vertex_map: &mut HashMap<VKey, usize>,
                 vertices: &mut Vec<threemf::model::mesh::Vertex>,
                 key: VKey|
   -> usize {
    *vertex_map.entry(key).or_insert_with(|| {
      let idx = vertices.len();
      let c = coord_map.get(&key).unwrap();
      vertices.push(threemf::model::mesh::Vertex {
        x: c[0] as f64,
        y: c[1] as f64,
        z: c[2] as f64,
      });
      idx
    })
  };

  for (keys, _coords) in &poly_data {
    let resolved = resolve_t_junctions(keys, &all_vkeys);
    let indices: Vec<usize> = resolved
      .iter()
      .map(|&k| get_idx(&mut vertex_map, &mut vertices, k))
      .collect();

    // Fan triangulate from first vertex
    for i in 1..indices.len().saturating_sub(1) {
      triangles.push(threemf::model::mesh::Triangle {
        v1: indices[0],
        v2: indices[i],
        v3: indices[i + 1],
      });
    }
  }

  let mesh = ThreemfMesh {
    vertices: threemf::model::mesh::Vertices { vertex: vertices },
    triangles: threemf::model::mesh::Triangles {
      triangle: triangles,
    },
  };

  let file = std::fs::File::create(path)
    .map_err(|e| format!("Failed to create file: {e}"))?;
  threemf::write(file, mesh)
    .map_err(|e| format!("Failed to write 3MF: {e}"))?;
  Ok(())
}

/// Merge all geometries into one csgrs mesh via union.
fn merge_geometries(geometries: &[CsgGeometry]) -> Result<CsgMesh<()>, String> {
  if geometries.is_empty() {
    return Err("No geometry to export".to_string());
  }
  let mut merged = geometries[0].mesh.clone();
  for geom in &geometries[1..] {
    if !geom.mesh.polygons.is_empty() {
      merged = merged.union(&geom.mesh);
    }
  }
  Ok(merged)
}

/// Export all geometries to a PLY file using csgrs's built-in PLY export.
pub fn export_ply(
  geometries: &[CsgGeometry],
  path: &std::path::Path,
) -> Result<(), String> {
  let merged = merge_geometries(geometries)?;
  let ply = merged.to_ply("Exported from LuaCAD Studio");
  std::fs::write(path, ply).map_err(|e| format!("Failed to write PLY: {e}"))?;
  Ok(())
}

/// Export all geometries to a binary STL file using csgrs's built-in STL export.
pub fn export_stl(
  geometries: &[CsgGeometry],
  path: &std::path::Path,
) -> Result<(), String> {
  let merged = merge_geometries(geometries)?;
  let stl_bytes = merged
    .to_stl_binary("LuaCAD Studio")
    .map_err(|e| format!("Failed to generate STL: {e}"))?;
  std::fs::write(path, stl_bytes)
    .map_err(|e| format!("Failed to write STL: {e}"))?;
  Ok(())
}

/// Export all geometries to an OBJ file using csgrs's built-in OBJ export.
pub fn export_obj(
  geometries: &[CsgGeometry],
  path: &std::path::Path,
) -> Result<(), String> {
  let merged = merge_geometries(geometries)?;
  let obj = merged.to_obj("LuaCAD_Studio");
  std::fs::write(path, obj).map_err(|e| format!("Failed to write OBJ: {e}"))?;
  Ok(())
}
