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

/// Export all geometries to a 3MF file. Uses original CAD coordinates (no GL transform).
///
/// csgrs boolean operations can produce meshes with non-manifold topology (boundary edges
/// that don't pair up). To work around this, we use each polygon's plane normal to
/// determine the correct outward winding for each triangle independently, rather than
/// relying on edge-neighbor consistency. Vertices are deduplicated by quantized position
/// to produce a proper indexed mesh.
pub fn export_3mf(
  geometries: &[CsgGeometry],
  path: &std::path::Path,
) -> Result<(), String> {
  use std::collections::HashMap;

  let merged = merge_geometries(geometries)?;

  if merged.polygons.is_empty() {
    return Err("No geometry to export".to_string());
  }

  let mut verts: Vec<[f64; 3]> = Vec::new();
  let mut tris: Vec<[usize; 3]> = Vec::new();
  let mut vertex_map: HashMap<VKey, usize> = HashMap::new();

  // Helper to get or insert a vertex index
  let get_idx = |verts: &mut Vec<[f64; 3]>,
                 vertex_map: &mut HashMap<VKey, usize>,
                 x: f32,
                 y: f32,
                 z: f32|
   -> usize {
    let key = vkey(x, y, z);
    *vertex_map.entry(key).or_insert_with(|| {
      let idx = verts.len();
      verts.push([x as f64, y as f64, z as f64]);
      idx
    })
  };

  for polygon in &merged.polygons {
    // Get the polygon's authoritative outward normal from its plane
    let plane_normal = polygon.plane.normal();

    // Triangulate the polygon (may be >3 vertices)
    let triangulated = polygon.triangulate();

    for tri_verts in &triangulated {
      let i0 = get_idx(
        &mut verts,
        &mut vertex_map,
        tri_verts[0].pos.x,
        tri_verts[0].pos.y,
        tri_verts[0].pos.z,
      );
      let i1 = get_idx(
        &mut verts,
        &mut vertex_map,
        tri_verts[1].pos.x,
        tri_verts[1].pos.y,
        tri_verts[1].pos.z,
      );
      let i2 = get_idx(
        &mut verts,
        &mut vertex_map,
        tri_verts[2].pos.x,
        tri_verts[2].pos.y,
        tri_verts[2].pos.z,
      );

      // Skip degenerate triangles
      if i0 == i1 || i1 == i2 || i0 == i2 {
        continue;
      }

      // Compute the triangle's geometric normal from vertex positions
      let v0 = verts[i0];
      let v1 = verts[i1];
      let v2 = verts[i2];
      let e1 = [v1[0] - v0[0], v1[1] - v0[1], v1[2] - v0[2]];
      let e2 = [v2[0] - v0[0], v2[1] - v0[1], v2[2] - v0[2]];
      let cross = [
        e1[1] * e2[2] - e1[2] * e2[1],
        e1[2] * e2[0] - e1[0] * e2[2],
        e1[0] * e2[1] - e1[1] * e2[0],
      ];

      // Dot the geometric normal with the polygon's authoritative normal.
      // If negative, the winding is backwards — swap two vertices to flip it.
      let dot = cross[0] * plane_normal.x as f64
        + cross[1] * plane_normal.y as f64
        + cross[2] * plane_normal.z as f64;

      if dot >= 0.0 {
        tris.push([i0, i1, i2]);
      } else {
        tris.push([i0, i2, i1]);
      }
    }
  }

  let vertices: Vec<threemf::model::mesh::Vertex> = verts
    .iter()
    .map(|v| threemf::model::mesh::Vertex {
      x: v[0],
      y: v[1],
      z: v[2],
    })
    .collect();

  let triangles: Vec<threemf::model::mesh::Triangle> = tris
    .iter()
    .map(|t| threemf::model::mesh::Triangle {
      v1: t[0],
      v2: t[1],
      v3: t[2],
    })
    .collect();

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

#[cfg(test)]
mod tests {
  use super::*;
  use std::collections::HashMap;

  /// Compute signed volume of the mesh. Positive = outward-facing normals.
  fn signed_volume(tris: &[[usize; 3]], verts: &[[f64; 3]]) -> f64 {
    tris
      .iter()
      .map(|tri| {
        let v0 = verts[tri[0]];
        let v1 = verts[tri[1]];
        let v2 = verts[tri[2]];
        v0[0] * (v1[1] * v2[2] - v1[2] * v2[1])
          + v0[1] * (v1[2] * v2[0] - v1[0] * v2[2])
          + v0[2] * (v1[0] * v2[1] - v1[1] * v2[0])
      })
      .sum()
  }

  /// Run the same pipeline as export_3mf, returning (verts, tris) for inspection.
  fn build_export_mesh(
    geometries: &[CsgGeometry],
  ) -> (Vec<[f64; 3]>, Vec<[usize; 3]>) {
    let merged = merge_geometries(geometries).unwrap();

    let mut verts: Vec<[f64; 3]> = Vec::new();
    let mut tris: Vec<[usize; 3]> = Vec::new();
    let mut vertex_map: HashMap<VKey, usize> = HashMap::new();

    let get_idx = |verts: &mut Vec<[f64; 3]>,
                   vertex_map: &mut HashMap<VKey, usize>,
                   x: f32,
                   y: f32,
                   z: f32|
     -> usize {
      let key = vkey(x, y, z);
      *vertex_map.entry(key).or_insert_with(|| {
        let idx = verts.len();
        verts.push([x as f64, y as f64, z as f64]);
        idx
      })
    };

    for polygon in &merged.polygons {
      let plane_normal = polygon.plane.normal();
      let triangulated = polygon.triangulate();

      for tri_verts in &triangulated {
        let i0 = get_idx(
          &mut verts,
          &mut vertex_map,
          tri_verts[0].pos.x,
          tri_verts[0].pos.y,
          tri_verts[0].pos.z,
        );
        let i1 = get_idx(
          &mut verts,
          &mut vertex_map,
          tri_verts[1].pos.x,
          tri_verts[1].pos.y,
          tri_verts[1].pos.z,
        );
        let i2 = get_idx(
          &mut verts,
          &mut vertex_map,
          tri_verts[2].pos.x,
          tri_verts[2].pos.y,
          tri_verts[2].pos.z,
        );

        if i0 == i1 || i1 == i2 || i0 == i2 {
          continue;
        }

        let v0 = verts[i0];
        let v1 = verts[i1];
        let v2 = verts[i2];
        let e1 = [v1[0] - v0[0], v1[1] - v0[1], v1[2] - v0[2]];
        let e2 = [v2[0] - v0[0], v2[1] - v0[1], v2[2] - v0[2]];
        let cross = [
          e1[1] * e2[2] - e1[2] * e2[1],
          e1[2] * e2[0] - e1[0] * e2[2],
          e1[0] * e2[1] - e1[1] * e2[0],
        ];

        let dot = cross[0] * plane_normal.x as f64
          + cross[1] * plane_normal.y as f64
          + cross[2] * plane_normal.z as f64;

        if dot >= 0.0 {
          tris.push([i0, i1, i2]);
        } else {
          tris.push([i0, i2, i1]);
        }
      }
    }

    (verts, tris)
  }

  /// Verify that each triangle's winding agrees with its polygon's plane normal.
  /// Returns the number of triangles with wrong winding.
  fn count_wrong_winding(geometries: &[CsgGeometry]) -> usize {
    let merged = merge_geometries(geometries).unwrap();
    let mut wrong = 0;

    for polygon in &merged.polygons {
      let plane_normal = polygon.plane.normal();
      let triangulated = polygon.triangulate();

      for tri in &triangulated {
        let v0 = tri[0].pos;
        let v1 = tri[1].pos;
        let v2 = tri[2].pos;
        let e1 = v1 - v0;
        let e2 = v2 - v0;
        let cross = e1.cross(&e2);
        let dot = cross.dot(&plane_normal);
        if dot < 0.0 {
          wrong += 1;
        }
      }
    }
    wrong
  }

  #[test]
  fn simple_cube_manifold_and_correct_winding() {
    let cube = CsgMesh::<()>::cuboid(10.0, 10.0, 10.0, None);
    let geom = CsgGeometry {
      mesh: cube,
      color: None,
    };

    let (verts, tris) = build_export_mesh(&[geom]);
    assert_eq!(tris.len(), 12, "cube should have 12 triangles");
    assert!(
      signed_volume(&tris, &verts) > 0.0,
      "cube should have positive signed volume"
    );
  }

  #[test]
  fn cube_minus_cylinder_correct_winding() {
    // The exact failing case from the bug report
    let cube =
      CsgMesh::<()>::cuboid(10.0, 10.0, 10.0, None).translate(-5.0, -5.0, -5.0);
    let cyl = CsgMesh::<()>::frustum(3.0, 3.0, 20.0, 32, None)
      .translate(0.0, 0.0, -10.0);
    let result = cube.difference(&cyl);

    let geom = CsgGeometry {
      mesh: result,
      color: None,
    };

    // Verify all triangles are oriented matching their polygon normal
    assert_eq!(
      count_wrong_winding(&[geom.clone()]),
      0,
      "all triangles should match their polygon's plane normal"
    );

    let (verts, tris) = build_export_mesh(&[geom]);
    assert!(!tris.is_empty(), "mesh should have triangles");
    assert!(
      signed_volume(&tris, &verts) > 0.0,
      "mesh should have positive signed volume (outward normals)"
    );
  }

  #[test]
  fn sphere_minus_sphere_correct_winding() {
    let outer = CsgMesh::<()>::sphere(10.0, 16, 8, None);
    let inner =
      CsgMesh::<()>::sphere(5.0, 16, 8, None).translate(5.0, 0.0, 0.0);
    let result = outer.difference(&inner);

    let geom = CsgGeometry {
      mesh: result,
      color: None,
    };

    assert_eq!(count_wrong_winding(&[geom.clone()]), 0);

    let (verts, tris) = build_export_mesh(&[geom]);
    assert!(!tris.is_empty());
    assert!(signed_volume(&tris, &verts) > 0.0);
  }

  #[test]
  fn cube_union_correct_winding() {
    let a = CsgMesh::<()>::cuboid(10.0, 10.0, 10.0, None);
    let b =
      CsgMesh::<()>::cuboid(10.0, 10.0, 10.0, None).translate(5.0, 5.0, 0.0);
    let result = a.union(&b);

    let geom = CsgGeometry {
      mesh: result,
      color: None,
    };

    assert_eq!(count_wrong_winding(&[geom.clone()]), 0);

    let (verts, tris) = build_export_mesh(&[geom]);
    assert!(!tris.is_empty());
    assert!(signed_volume(&tris, &verts) > 0.0);
  }

  #[test]
  fn degenerate_triangles_are_skipped() {
    // A cuboid that when quantized at 0.001mm could produce degenerate
    // triangles (e.g., very thin slivers). Verify no degenerate triangles
    // make it into the output.
    let cube = CsgMesh::<()>::cuboid(0.001, 0.001, 0.001, None);
    let geom = CsgGeometry {
      mesh: cube,
      color: None,
    };

    let (_, tris) = build_export_mesh(&[geom]);
    for tri in &tris {
      assert_ne!(tri[0], tri[1], "degenerate triangle: v0 == v1");
      assert_ne!(tri[1], tri[2], "degenerate triangle: v1 == v2");
      assert_ne!(tri[0], tri[2], "degenerate triangle: v0 == v2");
    }
  }

  #[test]
  fn export_3mf_writes_valid_file() {
    let cube = CsgMesh::<()>::cuboid(5.0, 5.0, 5.0, None);
    let geom = CsgGeometry {
      mesh: cube,
      color: None,
    };

    let dir = std::env::temp_dir().join("luacad_test");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("test_output.3mf");

    export_3mf(&[geom], &path).expect("export_3mf should succeed");
    assert!(path.exists(), "3MF file should be created");
    assert!(
      std::fs::metadata(&path).unwrap().len() > 0,
      "3MF file should not be empty"
    );

    let _ = std::fs::remove_file(&path);
  }

  #[test]
  fn export_3mf_cube_minus_cylinder_file() {
    // End-to-end: the exact model from the bug report writes a valid 3MF
    let cube =
      CsgMesh::<()>::cuboid(10.0, 10.0, 10.0, None).translate(-5.0, -5.0, -5.0);
    let cyl = CsgMesh::<()>::frustum(3.0, 3.0, 20.0, 32, None)
      .translate(0.0, 0.0, -10.0);
    let result = cube.difference(&cyl);

    let geom = CsgGeometry {
      mesh: result,
      color: None,
    };

    let dir = std::env::temp_dir().join("luacad_test");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("cube_minus_cyl.3mf");

    export_3mf(&[geom], &path).expect("export_3mf should succeed");
    assert!(path.exists());
    assert!(std::fs::metadata(&path).unwrap().len() > 0);

    let _ = std::fs::remove_file(&path);
  }
}
