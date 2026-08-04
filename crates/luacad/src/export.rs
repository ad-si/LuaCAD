use crate::geometry::CsgGeometry;
#[cfg(feature = "csgrs")]
use csgrs::mesh::Mesh as CsgMesh;
#[cfg(feature = "csgrs")]
use csgrs::traits::CSG;
#[cfg(feature = "csgrs")]
use std::collections::HashMap;
use threemf::Mesh as ThreemfMesh;

#[cfg(feature = "csgrs")]
#[derive(Debug, Clone, Copy)]
pub enum ExportFormat {
  ThreeMF,
  PLY,
  STL,
  OBJ,
  OpenSCAD,
}

#[cfg(feature = "csgrs")]
impl ExportFormat {
  pub const ALL: &[ExportFormat] = &[
    ExportFormat::ThreeMF,
    ExportFormat::STL,
    ExportFormat::OBJ,
    ExportFormat::PLY,
  ];

  pub fn label(self) -> &'static str {
    match self {
      Self::ThreeMF => "Export 3MF",
      Self::STL => "Export STL",
      Self::OBJ => "Export OBJ",
      Self::PLY => "Export PLY",
      Self::OpenSCAD => "Export SCAD",
    }
  }
}

/// Formats that OpenSCAD can export to via command-line.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum OpenScadFormat {
  Stl,
  ThreeMf,
  Off,
  Amf,
  Csg,
}

impl OpenScadFormat {
  pub const ALL: &[OpenScadFormat] = &[
    OpenScadFormat::Stl,
    OpenScadFormat::ThreeMf,
    OpenScadFormat::Off,
    OpenScadFormat::Amf,
    OpenScadFormat::Csg,
  ];

  pub fn label(self) -> &'static str {
    match self {
      Self::Stl => "Export STL",
      Self::ThreeMf => "Export 3MF",
      Self::Off => "Export OFF",
      Self::Amf => "Export AMF",
      Self::Csg => "Export CSG",
    }
  }

  pub fn extension(self) -> &'static str {
    match self {
      Self::Stl => "stl",
      Self::ThreeMf => "3mf",
      Self::Off => "off",
      Self::Amf => "amf",
      Self::Csg => "csg",
    }
  }

  pub fn filter_name(self) -> &'static str {
    match self {
      Self::Stl => "STL Files",
      Self::ThreeMf => "3MF Files",
      Self::Off => "OFF Files",
      Self::Amf => "AMF Files",
      Self::Csg => "CSG Files",
    }
  }

  pub fn default_filename(self) -> &'static str {
    match self {
      Self::Stl => "model.stl",
      Self::ThreeMf => "model.3mf",
      Self::Off => "model.off",
      Self::Amf => "model.amf",
      Self::Csg => "model.csg",
    }
  }
}

#[cfg(feature = "csgrs")]
type VKey = (i64, i64, i64);

/// Quantize a float to an integer key for vertex deduplication.
/// Uses micron precision (0.001mm) which is well beyond 3D printing accuracy.
#[cfg(feature = "csgrs")]
fn quantize(v: f32) -> i64 {
  (v as f64 * 1000.0).round() as i64
}

#[cfg(feature = "csgrs")]
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
#[cfg(feature = "csgrs")]
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

/// Export all geometries to 3MF bytes in memory.
#[cfg(feature = "csgrs")]
pub fn export_3mf_bytes(geometries: &[CsgGeometry]) -> Result<Vec<u8>, String> {
  use std::collections::HashMap;
  use std::io::Cursor;

  let merged = merge_geometries(geometries)?;

  if merged.polygons.is_empty() {
    return Err("No geometry to export".to_string());
  }

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

  let mut buf = Cursor::new(Vec::new());
  threemf::write(&mut buf, mesh)
    .map_err(|e| format!("Failed to write 3MF: {e}"))?;
  Ok(buf.into_inner())
}

/// Merge all geometries into one csgrs mesh via union.
/// Materializes lazy meshes from their ScadNode trees as needed.
#[cfg(feature = "csgrs")]
fn merge_geometries(geometries: &[CsgGeometry]) -> Result<CsgMesh<()>, String> {
  if geometries.is_empty() {
    return Err("No geometry to export".to_string());
  }
  // Materialize each geometry's mesh from its ScadNode if not already done.
  let meshes: Vec<CsgMesh<()>> = geometries
    .iter()
    .map(|geom| {
      if let Some(ref mesh) = geom.mesh {
        mesh.clone()
      } else if let Some(ref scad) = geom.scad {
        crate::geometry::materialize_scad(scad)
      } else {
        CsgMesh {
          polygons: vec![],
          bounding_box: std::sync::OnceLock::new(),
          metadata: None,
        }
      }
    })
    .collect();
  let mut merged = meshes[0].clone();
  for mesh in &meshes[1..] {
    if !mesh.polygons.is_empty() {
      merged = merged.union(mesh);
    }
  }
  Ok(merged)
}

/// Export all geometries to a PLY file using csgrs's built-in PLY export.
#[cfg(feature = "csgrs")]
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
#[cfg(feature = "csgrs")]
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

/// Export all geometries to an ASCII STL string using csgrs's built-in STL export.
#[cfg(feature = "csgrs")]
pub fn export_stl_ascii(
  geometries: &[CsgGeometry],
  name: &str,
) -> Result<String, String> {
  let merged = merge_geometries(geometries)?;
  Ok(merged.to_stl_ascii(name))
}

/// Export all geometries to an OBJ file using csgrs's built-in OBJ export.
#[cfg(feature = "csgrs")]
pub fn export_obj(
  geometries: &[CsgGeometry],
  path: &std::path::Path,
) -> Result<(), String> {
  let merged = merge_geometries(geometries)?;
  let obj = merged.to_obj("LuaCAD_Studio");
  std::fs::write(path, obj).map_err(|e| format!("Failed to write OBJ: {e}"))?;
  Ok(())
}

/// Export all geometries to an OFF file.
#[cfg(feature = "csgrs")]
pub fn export_off(
  geometries: &[CsgGeometry],
  path: &std::path::Path,
) -> Result<(), String> {
  let merged = merge_geometries(geometries)?;
  let (flat_verts, flat_tris) = mesh_to_flat_arrays(&merged);
  let n_verts = flat_verts.len() / 3;
  let n_tris = flat_tris.len() / 3;

  use std::io::Write;
  let mut file = std::fs::File::create(path)
    .map_err(|e| format!("Failed to create OFF file: {e}"))?;
  writeln!(file, "OFF").map_err(|e| format!("Failed to write OFF: {e}"))?;
  writeln!(file, "{n_verts} {n_tris} 0")
    .map_err(|e| format!("Failed to write OFF: {e}"))?;
  for i in 0..n_verts {
    let b = i * 3;
    writeln!(
      file,
      "{} {} {}",
      flat_verts[b],
      flat_verts[b + 1],
      flat_verts[b + 2]
    )
    .map_err(|e| format!("Failed to write OFF: {e}"))?;
  }
  for i in 0..n_tris {
    let b = i * 3;
    writeln!(
      file,
      "3 {} {} {}",
      flat_tris[b],
      flat_tris[b + 1],
      flat_tris[b + 2]
    )
    .map_err(|e| format!("Failed to write OFF: {e}"))?;
  }
  Ok(())
}

/// Export all geometries to an AMF file.
#[cfg(feature = "csgrs")]
pub fn export_amf(
  geometries: &[CsgGeometry],
  path: &std::path::Path,
) -> Result<(), String> {
  let merged = merge_geometries(geometries)?;
  let (flat_verts, flat_tris) = mesh_to_flat_arrays(&merged);
  let n_verts = flat_verts.len() / 3;
  let n_tris = flat_tris.len() / 3;

  use std::io::Write;
  let mut file = std::fs::File::create(path)
    .map_err(|e| format!("Failed to create AMF file: {e}"))?;
  writeln!(
    file,
    "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
     <amf unit=\"millimeter\" version=\"1.1\">\n\
     <object id=\"0\">\n\
     <mesh>"
  )
  .map_err(|e| format!("Failed to write AMF: {e}"))?;

  writeln!(file, "<vertices>")
    .map_err(|e| format!("Failed to write AMF: {e}"))?;
  for i in 0..n_verts {
    let b = i * 3;
    writeln!(
      file,
      "<vertex><coordinates><x>{}</x><y>{}</y><z>{}</z></coordinates></vertex>",
      flat_verts[b],
      flat_verts[b + 1],
      flat_verts[b + 2]
    )
    .map_err(|e| format!("Failed to write AMF: {e}"))?;
  }
  writeln!(file, "</vertices>")
    .map_err(|e| format!("Failed to write AMF: {e}"))?;

  writeln!(file, "<volume>")
    .map_err(|e| format!("Failed to write AMF: {e}"))?;
  for i in 0..n_tris {
    let b = i * 3;
    writeln!(
      file,
      "<triangle><v1>{}</v1><v2>{}</v2><v3>{}</v3></triangle>",
      flat_tris[b],
      flat_tris[b + 1],
      flat_tris[b + 2]
    )
    .map_err(|e| format!("Failed to write AMF: {e}"))?;
  }
  writeln!(file, "</volume>")
    .map_err(|e| format!("Failed to write AMF: {e}"))?;

  writeln!(file, "</mesh>\n</object>\n</amf>")
    .map_err(|e| format!("Failed to write AMF: {e}"))?;
  Ok(())
}

/// Formats supported by the Manifold export pipeline.
#[derive(Debug, Clone, Copy)]
pub enum ManifoldFormat {
  ThreeMF,
  Stl,
  Obj,
  Ply,
  Off,
  Amf,
}

impl ManifoldFormat {
  pub const ALL: &[ManifoldFormat] = &[
    ManifoldFormat::ThreeMF,
    ManifoldFormat::Stl,
    ManifoldFormat::Obj,
    ManifoldFormat::Ply,
    ManifoldFormat::Off,
    ManifoldFormat::Amf,
  ];

  /// All formats except ThreeMF, for the dropdown menu.
  pub const DROPDOWN: &[ManifoldFormat] = &[
    ManifoldFormat::Stl,
    ManifoldFormat::Obj,
    ManifoldFormat::Ply,
    ManifoldFormat::Off,
    ManifoldFormat::Amf,
  ];

  pub fn label(self) -> &'static str {
    match self {
      Self::ThreeMF => "Export 3MF",
      Self::Stl => "Export STL",
      Self::Obj => "Export OBJ",
      Self::Ply => "Export PLY",
      Self::Off => "Export OFF",
      Self::Amf => "Export AMF",
    }
  }

  pub fn extension(self) -> &'static str {
    match self {
      Self::ThreeMF => "3mf",
      Self::Stl => "stl",
      Self::Obj => "obj",
      Self::Ply => "ply",
      Self::Off => "off",
      Self::Amf => "amf",
    }
  }

  pub fn filter_name(self) -> &'static str {
    match self {
      Self::ThreeMF => "3MF Files",
      Self::Stl => "STL Files",
      Self::Obj => "OBJ Files",
      Self::Ply => "PLY Files",
      Self::Off => "OFF Files",
      Self::Amf => "AMF Files",
    }
  }

  pub fn default_filename(self) -> &'static str {
    match self {
      Self::ThreeMF => "model.3mf",
      Self::Stl => "model.stl",
      Self::Obj => "model.obj",
      Self::Ply => "model.ply",
      Self::Off => "model.off",
      Self::Amf => "model.amf",
    }
  }

  /// Parse a format string (file extension) into a ManifoldFormat.
  pub fn from_extension(ext: &str) -> Option<Self> {
    match ext {
      "3mf" => Some(Self::ThreeMF),
      "stl" => Some(Self::Stl),
      "obj" => Some(Self::Obj),
      "ply" => Some(Self::Ply),
      "off" => Some(Self::Off),
      "amf" => Some(Self::Amf),
      _ => None,
    }
  }
}

/// Extract deduplicated vertices and triangles from a csgrs mesh.
/// Returns (vertices as flat [x,y,z,...], triangle indices as flat [i0,i1,i2,...]).
#[cfg(feature = "csgrs")]
fn mesh_to_flat_arrays(mesh: &CsgMesh<()>) -> (Vec<f32>, Vec<u32>) {
  let mut verts: Vec<[f32; 3]> = Vec::new();
  let mut tris: Vec<[u32; 3]> = Vec::new();
  let mut vertex_map: HashMap<VKey, u32> = HashMap::new();

  let get_idx = |verts: &mut Vec<[f32; 3]>,
                 vertex_map: &mut HashMap<VKey, u32>,
                 x: f32,
                 y: f32,
                 z: f32|
   -> u32 {
    let key = vkey(x, y, z);
    *vertex_map.entry(key).or_insert_with(|| {
      let idx = verts.len() as u32;
      verts.push([x, y, z]);
      idx
    })
  };

  for polygon in &mesh.polygons {
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

      // Compute the triangle's geometric normal and compare with polygon plane normal
      let v0 = verts[i0 as usize];
      let v1 = verts[i1 as usize];
      let v2 = verts[i2 as usize];
      let e1 = [v1[0] - v0[0], v1[1] - v0[1], v1[2] - v0[2]];
      let e2 = [v2[0] - v0[0], v2[1] - v0[1], v2[2] - v0[2]];
      let cross = [
        e1[1] * e2[2] - e1[2] * e2[1],
        e1[2] * e2[0] - e1[0] * e2[2],
        e1[0] * e2[1] - e1[1] * e2[0],
      ];
      let dot = cross[0] * plane_normal.x
        + cross[1] * plane_normal.y
        + cross[2] * plane_normal.z;

      if dot >= 0.0 {
        tris.push([i0, i1, i2]);
      } else {
        tris.push([i0, i2, i1]);
      }
    }
  }

  // Flatten to interleaved arrays
  let flat_verts: Vec<f32> = verts.into_iter().flatten().collect();
  let flat_tris: Vec<u32> = tris.into_iter().flatten().collect();
  (flat_verts, flat_tris)
}

/// RAII wrapper around a ManifoldManifold pointer.
/// Automatically frees the pointer on drop.
pub struct Manifold(*mut manifold_sys::ManifoldManifold);

impl Manifold {
  fn alloc() -> *mut std::os::raw::c_void {
    unsafe {
      manifold_sys::manifold_alloc_manifold() as *mut std::os::raw::c_void
    }
  }

  fn empty() -> Self {
    Self(unsafe { manifold_sys::manifold_empty(Self::alloc()) })
  }

  fn ptr(&self) -> *mut manifold_sys::ManifoldManifold {
    self.0
  }

  fn is_empty(&self) -> bool {
    unsafe { manifold_sys::manifold_is_empty(self.0) != 0 }
  }

  /// Return the axis-aligned bounding box as (min, max) in [x, y, z].
  pub fn bounding_box(&self) -> ([f32; 3], [f32; 3]) {
    unsafe {
      let bbox = manifold_sys::manifold_bounding_box(
        manifold_sys::manifold_alloc_box() as *mut std::os::raw::c_void,
        self.0,
      );
      let mn = manifold_sys::manifold_box_min(bbox);
      let mx = manifold_sys::manifold_box_max(bbox);
      manifold_sys::manifold_delete_box(bbox);
      (
        [mn.x as f32, mn.y as f32, mn.z as f32],
        [mx.x as f32, mx.y as f32, mx.z as f32],
      )
    }
  }

  /// Return the number of triangles in the mesh.
  pub fn num_tri(&self) -> usize {
    unsafe { manifold_sys::manifold_num_tri(self.0) as usize }
  }

  /// Return the enclosed volume.
  pub fn volume(&self) -> f64 {
    unsafe { manifold_sys::manifold_volume(self.0) }
  }
}

impl Drop for Manifold {
  fn drop(&mut self) {
    if !self.0.is_null() {
      unsafe { manifold_sys::manifold_delete_manifold(self.0) };
    }
  }
}

/// RAII wrapper around a ManifoldPolygons pointer — the contour list that
/// `manifold_extrude` and `manifold_revolve` consume.
struct Polygons(*mut manifold_sys::ManifoldPolygons);

impl Polygons {
  fn ptr(&self) -> *mut manifold_sys::ManifoldPolygons {
    self.0
  }
}

impl Drop for Polygons {
  fn drop(&mut self) {
    if !self.0.is_null() {
      unsafe { manifold_sys::manifold_delete_polygons(self.0) };
    }
  }
}

/// RAII wrapper around a ManifoldCrossSection pointer — Manifold's 2D
/// counterpart to `Manifold`, used for `circle`/`square`/`polygon` and the
/// booleans and transforms applied to them before extrusion.
pub struct CrossSection(*mut manifold_sys::ManifoldCrossSection);

impl CrossSection {
  fn alloc() -> *mut std::os::raw::c_void {
    unsafe {
      manifold_sys::manifold_alloc_cross_section() as *mut std::os::raw::c_void
    }
  }

  fn empty() -> Self {
    Self(unsafe { manifold_sys::manifold_cross_section_empty(Self::alloc()) })
  }

  fn ptr(&self) -> *mut manifold_sys::ManifoldCrossSection {
    self.0
  }

  pub fn is_empty(&self) -> bool {
    unsafe { manifold_sys::manifold_cross_section_is_empty(self.0) != 0 }
  }

  /// Build a cross-section from a single closed contour.
  fn from_points(points: &[[f32; 2]]) -> Self {
    if points.len() < 3 {
      return Self::empty();
    }
    let mut verts: Vec<manifold_sys::ManifoldVec2> = points
      .iter()
      .map(|p| manifold_sys::ManifoldVec2 {
        x: p[0] as f64,
        y: p[1] as f64,
      })
      .collect();
    unsafe {
      // manifold_simple_polygon copies the points, so the temporary
      // contour is ours to free again.
      let simple = manifold_sys::manifold_simple_polygon(
        manifold_sys::manifold_alloc_simple_polygon()
          as *mut std::os::raw::c_void,
        verts.as_mut_ptr(),
        verts.len(),
      );
      let cs = Self(manifold_sys::manifold_cross_section_of_simple_polygon(
        Self::alloc(),
        simple,
        manifold_sys::ManifoldFillRule_MANIFOLD_FILL_RULE_NON_ZERO,
      ));
      manifold_sys::manifold_delete_simple_polygon(simple);
      cs
    }
  }

  /// Build a cross-section from multiple closed contours; holes are
  /// resolved with the non-zero fill rule.
  fn from_contours(contours: &[Vec<[f32; 2]>]) -> Self {
    let simples: Vec<*mut manifold_sys::ManifoldSimplePolygon> = contours
      .iter()
      .filter(|c| c.len() >= 3)
      .map(|contour| {
        let mut verts: Vec<manifold_sys::ManifoldVec2> = contour
          .iter()
          .map(|p| manifold_sys::ManifoldVec2 {
            x: p[0] as f64,
            y: p[1] as f64,
          })
          .collect();
        unsafe {
          manifold_sys::manifold_simple_polygon(
            manifold_sys::manifold_alloc_simple_polygon()
              as *mut std::os::raw::c_void,
            verts.as_mut_ptr(),
            verts.len(),
          )
        }
      })
      .collect();
    if simples.is_empty() {
      return Self::empty();
    }
    unsafe {
      let mut simples = simples;
      let polys = manifold_sys::manifold_polygons(
        manifold_sys::manifold_alloc_polygons() as *mut std::os::raw::c_void,
        simples.as_mut_ptr(),
        simples.len(),
      );
      let cs = Self(manifold_sys::manifold_cross_section_of_polygons(
        Self::alloc(),
        polys,
        manifold_sys::ManifoldFillRule_MANIFOLD_FILL_RULE_NON_ZERO,
      ));
      // Like manifold_simple_polygon, manifold_polygons copies its input,
      // so the temporaries are ours to free again
      manifold_sys::manifold_delete_polygons(polys);
      for simple in simples {
        manifold_sys::manifold_delete_simple_polygon(simple);
      }
      cs
    }
  }

  /// Build a cross-section from a contour list, e.g. the result of
  /// projecting or slicing a 3D manifold.
  fn from_polygons(polygons: &Polygons) -> Self {
    Self(unsafe {
      manifold_sys::manifold_cross_section_of_polygons(
        Self::alloc(),
        polygons.ptr(),
        manifold_sys::ManifoldFillRule_MANIFOLD_FILL_RULE_NON_ZERO,
      )
    })
  }

  fn to_polygons(&self) -> Polygons {
    Polygons(unsafe {
      manifold_sys::manifold_cross_section_to_polygons(
        manifold_sys::manifold_alloc_polygons() as *mut std::os::raw::c_void,
        self.0,
      )
    })
  }
}

impl Drop for CrossSection {
  fn drop(&mut self) {
    if !self.0.is_null() {
      unsafe { manifold_sys::manifold_delete_cross_section(self.0) };
    }
  }
}

/// Convert a single csgrs mesh into a Manifold object via FFI.
#[cfg(feature = "csgrs")]
fn csg_mesh_to_manifold(mesh: &CsgMesh<()>) -> Result<Manifold, String> {
  use manifold_sys::*;
  use std::os::raw::c_void;

  let (mut vert_props, mut tri_indices) = mesh_to_flat_arrays(mesh);
  let n_verts = vert_props.len() / 3;
  let n_tris = tri_indices.len() / 3;

  unsafe {
    let mesh_gl = manifold_meshgl(
      manifold_alloc_meshgl() as *mut c_void,
      vert_props.as_mut_ptr(),
      n_verts,
      3,
      tri_indices.as_mut_ptr(),
      n_tris,
    );

    let m = Manifold(manifold_of_meshgl(Manifold::alloc(), mesh_gl));
    manifold_delete_meshgl(mesh_gl);

    let status = manifold_status(m.ptr());
    if status != ManifoldError_MANIFOLD_NO_ERROR {
      #[allow(non_upper_case_globals)]
      let err = match status {
        ManifoldError_MANIFOLD_NON_FINITE_VERTEX => "non-finite vertex",
        ManifoldError_MANIFOLD_NOT_MANIFOLD => "not manifold",
        ManifoldError_MANIFOLD_VERTEX_INDEX_OUT_OF_BOUNDS => {
          "vertex index out of bounds"
        }
        ManifoldError_MANIFOLD_PROPERTIES_WRONG_LENGTH => {
          "properties wrong length"
        }
        ManifoldError_MANIFOLD_MISSING_POSITION_PROPERTIES => {
          "missing position properties"
        }
        _ => "unknown error",
      };
      return Err(format!("Manifold construction failed: {err}"));
    }

    Ok(m)
  }
}

/// Recursively evaluate a ScadNode tree into a Manifold object.
/// All boolean operations, transforms, and primitives are performed
/// directly by the Manifold library — no csgrs involved.
pub fn materialize_scad_manifold(
  node: &crate::scad_export::ScadNode,
) -> Manifold {
  use crate::scad_export::ScadNode;
  use manifold_sys::*;

  match node {
    // --- Leaf 3D primitives ---
    ScadNode::Cube { w, d, h, center } => Manifold(unsafe {
      manifold_cube(
        Manifold::alloc(),
        *w as f64,
        *d as f64,
        *h as f64,
        *center as i32,
      )
    }),

    ScadNode::Sphere { r, segments } => Manifold(unsafe {
      manifold_sphere(Manifold::alloc(), *r as f64, *segments as i32)
    }),

    ScadNode::Cylinder {
      r1,
      r2,
      h,
      segments,
      center,
    } => Manifold(unsafe {
      manifold_cylinder(
        Manifold::alloc(),
        *h as f64,
        *r1 as f64,
        *r2 as f64,
        *segments as i32,
        *center as i32,
      )
    }),

    ScadNode::Polyhedron { points, faces } => {
      use manifold_sys::*;
      use std::os::raw::c_void;

      // Fan-triangulate faces and build flat arrays
      let mut flat_verts: Vec<f32> = Vec::new();
      let mut flat_tris: Vec<u32> = Vec::new();

      for p in points {
        flat_verts.push(p[0]);
        flat_verts.push(p[1]);
        flat_verts.push(p[2]);
      }

      for face in faces {
        if face.len() < 3 {
          continue;
        }
        // Fan triangulation from first vertex
        for i in 1..face.len() - 1 {
          flat_tris.push(face[0] as u32);
          flat_tris.push(face[i] as u32);
          flat_tris.push(face[i + 1] as u32);
        }
      }

      let n_verts = points.len();
      let n_tris = flat_tris.len() / 3;

      if n_verts == 0 || n_tris == 0 {
        return Manifold::empty();
      }

      unsafe {
        let mesh_gl = manifold_meshgl(
          manifold_alloc_meshgl() as *mut c_void,
          flat_verts.as_mut_ptr(),
          n_verts,
          3,
          flat_tris.as_mut_ptr(),
          n_tris,
        );
        let m = Manifold(manifold_of_meshgl(Manifold::alloc(), mesh_gl));
        manifold_delete_meshgl(mesh_gl);

        let status = manifold_status(m.ptr());
        if status != ManifoldError_MANIFOLD_NO_ERROR {
          Manifold::empty()
        } else {
          m
        }
      }
    }

    // --- CSG booleans ---
    ScadNode::Union(children) => {
      let mut iter = children.iter();
      let first = iter
        .next()
        .map(materialize_scad_manifold)
        .unwrap_or_else(Manifold::empty);
      iter.fold(first, |acc, child| {
        let next = materialize_scad_manifold(child);

        // acc and next are dropped here, freeing old pointers
        Manifold(unsafe {
          manifold_union(Manifold::alloc(), acc.ptr(), next.ptr())
        })
      })
    }

    ScadNode::Difference(children) => {
      // `*`/`%` children are removed from the boolean entirely, so the
      // base is the first remaining child (OpenSCAD semantics).
      let mut iter = children.iter().filter(|c| !c.is_csg_dropped());
      let first = iter
        .next()
        .map(materialize_scad_manifold)
        .unwrap_or_else(Manifold::empty);
      iter.fold(first, |acc, child| {
        let next = materialize_scad_manifold(child);

        Manifold(unsafe {
          manifold_difference(Manifold::alloc(), acc.ptr(), next.ptr())
        })
      })
    }

    ScadNode::Intersection(children) => {
      // `*`/`%` children are removed from the boolean entirely; keeping
      // them would intersect with an empty manifold and erase the result.
      let mut iter = children.iter().filter(|c| !c.is_csg_dropped());
      let first = iter
        .next()
        .map(materialize_scad_manifold)
        .unwrap_or_else(Manifold::empty);
      iter.fold(first, |acc, child| {
        let next = materialize_scad_manifold(child);

        Manifold(unsafe {
          manifold_intersection(Manifold::alloc(), acc.ptr(), next.ptr())
        })
      })
    }

    ScadNode::Hull(child) => {
      let m = materialize_scad_manifold(child);
      Manifold(unsafe { manifold_hull(Manifold::alloc(), m.ptr()) })
    }

    ScadNode::Minkowski(children) => {
      let mut iter = children.iter();
      let first = iter
        .next()
        .map(materialize_scad_manifold)
        .unwrap_or_else(Manifold::empty);
      iter.fold(first, |acc, child| {
        let next = materialize_scad_manifold(child);

        Manifold(unsafe {
          manifold_minkowski_sum(Manifold::alloc(), acc.ptr(), next.ptr())
        })
      })
    }

    // --- Transforms ---
    ScadNode::Translate { x, y, z, child } => {
      let m = materialize_scad_manifold(child);
      Manifold(unsafe {
        manifold_translate(
          Manifold::alloc(),
          m.ptr(),
          *x as f64,
          *y as f64,
          *z as f64,
        )
      })
    }

    ScadNode::Rotate { x, y, z, child } => {
      let m = materialize_scad_manifold(child);
      Manifold(unsafe {
        manifold_rotate(
          Manifold::alloc(),
          m.ptr(),
          *x as f64,
          *y as f64,
          *z as f64,
        )
      })
    }

    ScadNode::Scale { x, y, z, child } => {
      let m = materialize_scad_manifold(child);
      Manifold(unsafe {
        manifold_scale(
          Manifold::alloc(),
          m.ptr(),
          *x as f64,
          *y as f64,
          *z as f64,
        )
      })
    }

    ScadNode::Mirror { x, y, z, child } => {
      let m = materialize_scad_manifold(child);
      Manifold(unsafe {
        manifold_mirror(
          Manifold::alloc(),
          m.ptr(),
          *x as f64,
          *y as f64,
          *z as f64,
        )
      })
    }

    ScadNode::Multmatrix { matrix, child } => {
      let m = materialize_scad_manifold(child);
      // Manifold's transform takes a 4×3 matrix (row-major):
      // Row 0: [m[0], m[1], m[2]]     (x-axis + translate.x = m[3] goes in row 3)
      // Row 1: [m[4], m[5], m[6]]
      // Row 2: [m[8], m[9], m[10]]
      // Row 3: [m[3], m[7], m[11]]     (translation column)
      //
      // But the C API parameter order is:
      // x1,y1,z1 (column 0), x2,y2,z2 (column 1), x3,y3,z3 (column 2), x4,y4,z4 (translation)
      Manifold(unsafe {
        manifold_transform(
          Manifold::alloc(),
          m.ptr(),
          matrix[0] as f64,
          matrix[4] as f64,
          matrix[8] as f64, // col 0
          matrix[1] as f64,
          matrix[5] as f64,
          matrix[9] as f64, // col 1
          matrix[2] as f64,
          matrix[6] as f64,
          matrix[10] as f64, // col 2
          matrix[3] as f64,
          matrix[7] as f64,
          matrix[11] as f64, // translation
        )
      })
    }

    ScadNode::Resize { x, y, z, child } => {
      let m = materialize_scad_manifold(child);
      if m.is_empty() {
        return m;
      }
      unsafe {
        let bbox = manifold_bounding_box(
          manifold_alloc_box() as *mut std::os::raw::c_void,
          m.ptr(),
        );
        let mn = manifold_box_min(bbox);
        let mx = manifold_box_max(bbox);
        manifold_delete_box(bbox);

        let cur_w = mx.x - mn.x;
        let cur_d = mx.y - mn.y;
        let cur_h = mx.z - mn.z;

        let sx = if *x > 0.0 && cur_w > 1e-9 {
          *x as f64 / cur_w
        } else {
          1.0
        };
        let sy = if *y > 0.0 && cur_d > 1e-9 {
          *y as f64 / cur_d
        } else {
          1.0
        };
        let sz = if *z > 0.0 && cur_h > 1e-9 {
          *z as f64 / cur_h
        } else {
          1.0
        };

        Manifold(manifold_scale(Manifold::alloc(), m.ptr(), sx, sy, sz))
      }
    }

    // --- Extrusions: 2D cross-section lifted into 3D ---
    ScadNode::LinearExtrude {
      height,
      center,
      twist,
      slices,
      scale,
      child,
    } => {
      let cs = materialize_scad_cross_section(child);
      if cs.is_empty() {
        return Manifold::empty();
      }
      // OpenSCAD leaves `slices` at 0 to mean "pick a sensible number";
      // without intermediate slices a twist would just shear the sides.
      let divisions = if *slices > 0 {
        *slices as i32
      } else if *twist != 0.0 {
        (twist.abs() / 2.0).ceil().max(1.0) as i32
      } else {
        0
      };
      let polygons = cs.to_polygons();
      let m = Manifold(unsafe {
        manifold_extrude(
          Manifold::alloc(),
          polygons.ptr(),
          *height as f64,
          divisions,
          // OpenSCAD twists the top clockwise, Manifold counter-clockwise
          -*twist as f64,
          *scale as f64,
          *scale as f64,
        )
      });
      if *center {
        Manifold(unsafe {
          manifold_translate(
            Manifold::alloc(),
            m.ptr(),
            0.0,
            0.0,
            -*height as f64 / 2.0,
          )
        })
      } else {
        m
      }
    }

    ScadNode::RotateExtrude {
      angle,
      segments,
      child,
    } => {
      let cs = materialize_scad_cross_section(child);
      if cs.is_empty() {
        return Manifold::empty();
      }
      let polygons = cs.to_polygons();
      Manifold(unsafe {
        manifold_revolve(
          Manifold::alloc(),
          polygons.ptr(),
          *segments as i32,
          *angle as f64,
        )
      })
    }

    // --- Color / render: pass through ---
    ScadNode::Color { child, .. } | ScadNode::Render { child, .. } => {
      materialize_scad_manifold(child)
    }

    // --- Modifiers ---
    // `*` (disable) and `%` (background) are excluded from the CSG result;
    // `#` (debug) and `!` (root) don't change the geometry itself.
    ScadNode::Modifier { kind, child } => match kind {
      crate::scad_export::ModifierKind::Skip
      | crate::scad_export::ModifierKind::Transparent => Manifold::empty(),
      crate::scad_export::ModifierKind::Debug
      | crate::scad_export::ModifierKind::Only => {
        materialize_scad_manifold(child)
      }
    },

    // --- 2D shapes (only meaningful under an extrusion), text
    // and file ops: not yet supported ---
    _ => Manifold::empty(),
  }
}

/// Recursively evaluate the 2D part of a ScadNode tree into a Manifold
/// CrossSection, mirroring [`materialize_scad_manifold`] for sketches.
/// Nodes that aren't 2D — or aren't supported yet, such as `text()` —
/// yield an empty cross-section.
pub fn materialize_scad_cross_section(
  node: &crate::scad_export::ScadNode,
) -> CrossSection {
  use crate::scad_export::ScadNode;
  use manifold_sys::*;

  match node {
    // --- Leaf 2D primitives ---
    ScadNode::Circle { r, segments } => CrossSection(unsafe {
      manifold_cross_section_circle(
        CrossSection::alloc(),
        *r as f64,
        *segments as i32,
      )
    }),

    ScadNode::Square { w, h, center } => CrossSection(unsafe {
      manifold_cross_section_square(
        CrossSection::alloc(),
        *w as f64,
        *h as f64,
        *center as i32,
      )
    }),

    ScadNode::Polygon { points } => CrossSection::from_points(points),

    ScadNode::Import { file, .. } if file.to_lowercase().ends_with(".svg") => {
      match crate::svg_import::svg_to_polygons(file) {
        Ok(contours) => CrossSection::from_contours(&contours),
        Err(e) => {
          eprintln!("Warning: {e}");
          CrossSection::empty()
        }
      }
    }

    // --- CSG booleans ---
    ScadNode::Union(children) => {
      let mut iter = children.iter();
      let first = iter
        .next()
        .map(materialize_scad_cross_section)
        .unwrap_or_else(CrossSection::empty);
      iter.fold(first, |acc, child| {
        let next = materialize_scad_cross_section(child);
        CrossSection(unsafe {
          manifold_cross_section_union(
            CrossSection::alloc(),
            acc.ptr(),
            next.ptr(),
          )
        })
      })
    }

    ScadNode::Difference(children) => {
      let mut iter = children.iter();
      let first = iter
        .next()
        .map(materialize_scad_cross_section)
        .unwrap_or_else(CrossSection::empty);
      iter.fold(first, |acc, child| {
        let next = materialize_scad_cross_section(child);
        CrossSection(unsafe {
          manifold_cross_section_difference(
            CrossSection::alloc(),
            acc.ptr(),
            next.ptr(),
          )
        })
      })
    }

    ScadNode::Intersection(children) => {
      let mut iter = children.iter();
      let first = iter
        .next()
        .map(materialize_scad_cross_section)
        .unwrap_or_else(CrossSection::empty);
      iter.fold(first, |acc, child| {
        let next = materialize_scad_cross_section(child);
        CrossSection(unsafe {
          manifold_cross_section_intersection(
            CrossSection::alloc(),
            acc.ptr(),
            next.ptr(),
          )
        })
      })
    }

    ScadNode::Hull(child) => {
      let cs = materialize_scad_cross_section(child);
      CrossSection(unsafe {
        manifold_cross_section_hull(CrossSection::alloc(), cs.ptr())
      })
    }

    // --- Transforms (the Z component is meaningless in 2D) ---
    ScadNode::Translate { x, y, child, .. } => {
      let cs = materialize_scad_cross_section(child);
      CrossSection(unsafe {
        manifold_cross_section_translate(
          CrossSection::alloc(),
          cs.ptr(),
          *x as f64,
          *y as f64,
        )
      })
    }

    ScadNode::Rotate { z, child, .. } => {
      let cs = materialize_scad_cross_section(child);
      CrossSection(unsafe {
        manifold_cross_section_rotate(
          CrossSection::alloc(),
          cs.ptr(),
          *z as f64,
        )
      })
    }

    ScadNode::Scale { x, y, child, .. } => {
      let cs = materialize_scad_cross_section(child);
      CrossSection(unsafe {
        manifold_cross_section_scale(
          CrossSection::alloc(),
          cs.ptr(),
          *x as f64,
          *y as f64,
        )
      })
    }

    ScadNode::Mirror { x, y, child, .. } => {
      let cs = materialize_scad_cross_section(child);
      CrossSection(unsafe {
        manifold_cross_section_mirror(
          CrossSection::alloc(),
          cs.ptr(),
          *x as f64,
          *y as f64,
        )
      })
    }

    ScadNode::Offset {
      delta,
      r,
      chamfer,
      child,
    } => {
      let cs = materialize_scad_cross_section(child);
      // `r` rounds the corners, `chamfer` cuts them off, and plain `delta`
      // extends the edges until they meet.
      let (dist, join) = if let Some(r) = r {
        (*r, ManifoldJoinType_MANIFOLD_JOIN_TYPE_ROUND)
      } else if let Some(d) = delta {
        if *chamfer {
          (*d, ManifoldJoinType_MANIFOLD_JOIN_TYPE_SQUARE)
        } else {
          (*d, ManifoldJoinType_MANIFOLD_JOIN_TYPE_MITER)
        }
      } else {
        return cs;
      };
      CrossSection(unsafe {
        manifold_cross_section_offset(
          CrossSection::alloc(),
          cs.ptr(),
          dist as f64,
          join,
          2.0,
          0,
        )
      })
    }

    // --- 3D → 2D ---
    ScadNode::Projection { cut, child } => {
      let m = materialize_scad_manifold(child);
      if m.is_empty() {
        return CrossSection::empty();
      }
      let polygons = Polygons(unsafe {
        let mem = manifold_alloc_polygons() as *mut std::os::raw::c_void;
        if *cut {
          manifold_slice(mem, m.ptr(), 0.0)
        } else {
          manifold_project(mem, m.ptr())
        }
      });
      CrossSection::from_polygons(&polygons)
    }

    // --- Color / render: pass through ---
    ScadNode::Color { child, .. } | ScadNode::Render { child, .. } => {
      materialize_scad_cross_section(child)
    }

    // --- Modifiers: `*`/`%` are excluded, `#`/`!` pass through ---
    ScadNode::Modifier { kind, child } => match kind {
      crate::scad_export::ModifierKind::Skip
      | crate::scad_export::ModifierKind::Transparent => CrossSection::empty(),
      crate::scad_export::ModifierKind::Debug
      | crate::scad_export::ModifierKind::Only => {
        materialize_scad_cross_section(child)
      }
    },

    // --- 3D shapes, text and file ops: nothing to extrude ---
    _ => CrossSection::empty(),
  }
}

/// Extracted triangle mesh from a Manifold object.
pub struct ManifoldMesh {
  pub vertices: Vec<[f32; 3]>,
  pub triangles: Vec<[u32; 3]>,
}

/// Extract the triangle mesh (vertices + triangles) from a Manifold object.
pub fn extract_manifold_mesh(manifold: &Manifold) -> ManifoldMesh {
  use manifold_sys::*;
  use std::alloc::{Layout, alloc};
  use std::os::raw::c_void;

  unsafe {
    let out_mesh = manifold_get_meshgl(
      manifold_alloc_meshgl() as *mut c_void,
      manifold.ptr(),
    );

    let n_verts = manifold_meshgl_num_vert(out_mesh) as usize;
    let n_tris = manifold_meshgl_num_tri(out_mesh) as usize;
    let n_props = manifold_meshgl_num_prop(out_mesh) as usize;

    // Read vertex properties (interleaved: x,y,z + extra props per vertex)
    let vert_count = n_verts * n_props;
    let vert_layout = Layout::array::<f32>(vert_count).unwrap();
    let vert_ptr = alloc(vert_layout) as *mut f32;
    manifold_meshgl_vert_properties(vert_ptr as *mut c_void, out_mesh);
    let raw_verts = Vec::from_raw_parts(vert_ptr, vert_count, vert_count);

    // Read triangle indices
    let tri_count = n_tris * 3;
    let tri_layout = Layout::array::<u32>(tri_count).unwrap();
    let tri_ptr = alloc(tri_layout) as *mut u32;
    manifold_meshgl_tri_verts(tri_ptr as *mut c_void, out_mesh);
    let raw_tris = Vec::from_raw_parts(tri_ptr, tri_count, tri_count);

    manifold_delete_meshgl(out_mesh);

    let vertices: Vec<[f32; 3]> = (0..n_verts)
      .map(|i| {
        let base = i * n_props;
        [raw_verts[base], raw_verts[base + 1], raw_verts[base + 2]]
      })
      .collect();

    let triangles: Vec<[u32; 3]> = (0..n_tris)
      .map(|i| {
        let base = i * 3;
        [raw_tris[base], raw_tris[base + 1], raw_tris[base + 2]]
      })
      .collect();

    ManifoldMesh {
      vertices,
      triangles,
    }
  }
}

/// Write a ManifoldMesh as a 3MF file.
fn write_manifold_3mf(
  mesh: &ManifoldMesh,
  path: &std::path::Path,
) -> Result<(), String> {
  let vertices: Vec<threemf::model::mesh::Vertex> = mesh
    .vertices
    .iter()
    .map(|v| threemf::model::mesh::Vertex {
      x: v[0] as f64,
      y: v[1] as f64,
      z: v[2] as f64,
    })
    .collect();

  let triangles: Vec<threemf::model::mesh::Triangle> = mesh
    .triangles
    .iter()
    .map(|t| threemf::model::mesh::Triangle {
      v1: t[0] as usize,
      v2: t[1] as usize,
      v3: t[2] as usize,
    })
    .collect();

  let threemf_mesh = ThreemfMesh {
    vertices: threemf::model::mesh::Vertices { vertex: vertices },
    triangles: threemf::model::mesh::Triangles {
      triangle: triangles,
    },
  };

  let file = std::fs::File::create(path)
    .map_err(|e| format!("Failed to create file: {e}"))?;
  threemf::write(file, threemf_mesh)
    .map_err(|e| format!("Failed to write 3MF: {e}"))?;
  Ok(())
}

/// Write a ManifoldMesh as a binary STL file.
fn write_manifold_stl(
  mesh: &ManifoldMesh,
  path: &std::path::Path,
) -> Result<(), String> {
  use std::io::Write;

  let n_tris = mesh.triangles.len() as u32;
  let mut buf = Vec::with_capacity(84 + (n_tris as usize) * 50);

  // 80-byte header
  let header = b"LuaCAD Manifold STL export";
  buf.extend_from_slice(header);
  buf.extend_from_slice(&[0u8; 80 - 26]); // pad to 80 bytes
  buf.extend_from_slice(&n_tris.to_le_bytes());

  for tri in &mesh.triangles {
    let v0 = mesh.vertices[tri[0] as usize];
    let v1 = mesh.vertices[tri[1] as usize];
    let v2 = mesh.vertices[tri[2] as usize];

    // Compute face normal
    let e1 = [v1[0] - v0[0], v1[1] - v0[1], v1[2] - v0[2]];
    let e2 = [v2[0] - v0[0], v2[1] - v0[1], v2[2] - v0[2]];
    let nx = e1[1] * e2[2] - e1[2] * e2[1];
    let ny = e1[2] * e2[0] - e1[0] * e2[2];
    let nz = e1[0] * e2[1] - e1[1] * e2[0];
    let len = (nx * nx + ny * ny + nz * nz).sqrt();
    let (nx, ny, nz) = if len > 0.0 {
      (nx / len, ny / len, nz / len)
    } else {
      (0.0, 0.0, 0.0)
    };

    // Normal
    buf.extend_from_slice(&nx.to_le_bytes());
    buf.extend_from_slice(&ny.to_le_bytes());
    buf.extend_from_slice(&nz.to_le_bytes());
    // Vertices
    for v in [v0, v1, v2] {
      buf.extend_from_slice(&v[0].to_le_bytes());
      buf.extend_from_slice(&v[1].to_le_bytes());
      buf.extend_from_slice(&v[2].to_le_bytes());
    }
    // Attribute byte count
    buf.extend_from_slice(&0u16.to_le_bytes());
  }

  let mut file = std::fs::File::create(path)
    .map_err(|e| format!("Failed to create STL file: {e}"))?;
  file
    .write_all(&buf)
    .map_err(|e| format!("Failed to write STL: {e}"))?;
  Ok(())
}

/// Write a ManifoldMesh as an OBJ file.
fn write_manifold_obj(
  mesh: &ManifoldMesh,
  path: &std::path::Path,
) -> Result<(), String> {
  use std::io::Write;

  let mut file = std::fs::File::create(path)
    .map_err(|e| format!("Failed to create OBJ file: {e}"))?;

  writeln!(file, "# LuaCAD Manifold OBJ export")
    .map_err(|e| format!("Failed to write OBJ: {e}"))?;

  for v in &mesh.vertices {
    writeln!(file, "v {} {} {}", v[0], v[1], v[2])
      .map_err(|e| format!("Failed to write OBJ: {e}"))?;
  }

  // OBJ uses 1-based indices
  for tri in &mesh.triangles {
    writeln!(file, "f {} {} {}", tri[0] + 1, tri[1] + 1, tri[2] + 1)
      .map_err(|e| format!("Failed to write OBJ: {e}"))?;
  }

  Ok(())
}

/// Write a ManifoldMesh as a PLY file (binary little-endian).
fn write_manifold_ply(
  mesh: &ManifoldMesh,
  path: &std::path::Path,
) -> Result<(), String> {
  use std::io::Write;

  let mut file = std::fs::File::create(path)
    .map_err(|e| format!("Failed to create PLY file: {e}"))?;

  // PLY header
  write!(
    file,
    "ply\n\
     format binary_little_endian 1.0\n\
     comment LuaCAD Manifold export\n\
     element vertex {}\n\
     property float x\n\
     property float y\n\
     property float z\n\
     element face {}\n\
     property list uchar uint vertex_indices\n\
     end_header\n",
    mesh.vertices.len(),
    mesh.triangles.len()
  )
  .map_err(|e| format!("Failed to write PLY header: {e}"))?;

  // Vertex data
  for v in &mesh.vertices {
    file
      .write_all(&v[0].to_le_bytes())
      .map_err(|e| format!("Failed to write PLY: {e}"))?;
    file
      .write_all(&v[1].to_le_bytes())
      .map_err(|e| format!("Failed to write PLY: {e}"))?;
    file
      .write_all(&v[2].to_le_bytes())
      .map_err(|e| format!("Failed to write PLY: {e}"))?;
  }

  // Face data
  for tri in &mesh.triangles {
    file
      .write_all(&[3u8])
      .map_err(|e| format!("Failed to write PLY: {e}"))?;
    file
      .write_all(&tri[0].to_le_bytes())
      .map_err(|e| format!("Failed to write PLY: {e}"))?;
    file
      .write_all(&tri[1].to_le_bytes())
      .map_err(|e| format!("Failed to write PLY: {e}"))?;
    file
      .write_all(&tri[2].to_le_bytes())
      .map_err(|e| format!("Failed to write PLY: {e}"))?;
  }

  Ok(())
}

/// Write a ManifoldMesh as an OFF (Object File Format) file.
fn write_manifold_off(
  mesh: &ManifoldMesh,
  path: &std::path::Path,
) -> Result<(), String> {
  use std::io::Write;

  let mut file = std::fs::File::create(path)
    .map_err(|e| format!("Failed to create OFF file: {e}"))?;

  // OFF header: OFF\n nVertices nFaces nEdges
  writeln!(file, "OFF").map_err(|e| format!("Failed to write OFF: {e}"))?;
  writeln!(file, "{} {} 0", mesh.vertices.len(), mesh.triangles.len())
    .map_err(|e| format!("Failed to write OFF: {e}"))?;

  for v in &mesh.vertices {
    writeln!(file, "{} {} {}", v[0], v[1], v[2])
      .map_err(|e| format!("Failed to write OFF: {e}"))?;
  }

  for tri in &mesh.triangles {
    writeln!(file, "3 {} {} {}", tri[0], tri[1], tri[2])
      .map_err(|e| format!("Failed to write OFF: {e}"))?;
  }

  Ok(())
}

/// Write a ManifoldMesh as an AMF (Additive Manufacturing File Format) file.
fn write_manifold_amf(
  mesh: &ManifoldMesh,
  path: &std::path::Path,
) -> Result<(), String> {
  use std::io::Write;

  let mut file = std::fs::File::create(path)
    .map_err(|e| format!("Failed to create AMF file: {e}"))?;

  writeln!(
    file,
    "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
     <amf unit=\"millimeter\" version=\"1.1\">\n\
     <object id=\"0\">\n\
     <mesh>"
  )
  .map_err(|e| format!("Failed to write AMF: {e}"))?;

  // Vertices
  writeln!(file, "<vertices>")
    .map_err(|e| format!("Failed to write AMF: {e}"))?;
  for v in &mesh.vertices {
    writeln!(
      file,
      "<vertex><coordinates><x>{}</x><y>{}</y><z>{}</z></coordinates></vertex>",
      v[0], v[1], v[2]
    )
    .map_err(|e| format!("Failed to write AMF: {e}"))?;
  }
  writeln!(file, "</vertices>")
    .map_err(|e| format!("Failed to write AMF: {e}"))?;

  // Volume (triangles)
  writeln!(file, "<volume>")
    .map_err(|e| format!("Failed to write AMF: {e}"))?;
  for tri in &mesh.triangles {
    writeln!(
      file,
      "<triangle><v1>{}</v1><v2>{}</v2><v3>{}</v3></triangle>",
      tri[0], tri[1], tri[2]
    )
    .map_err(|e| format!("Failed to write AMF: {e}"))?;
  }
  writeln!(file, "</volume>")
    .map_err(|e| format!("Failed to write AMF: {e}"))?;

  writeln!(file, "</mesh>\n</object>\n</amf>")
    .map_err(|e| format!("Failed to write AMF: {e}"))?;

  Ok(())
}

/// Export geometries entirely via the Manifold library.
///
/// Walks each geometry's ScadNode tree, performing all boolean operations,
/// transforms, and primitive construction directly in Manifold. This
/// produces guaranteed valid 2-manifold output (watertight, properly
/// oriented) without involving csgrs for booleans.
pub fn export_manifold(
  geometries: &[CsgGeometry],
  format: &str,
  path: &std::path::Path,
) -> Result<(), String> {
  let fmt = ManifoldFormat::from_extension(format).ok_or_else(|| {
    format!(
      "Unsupported manifold format: {format}\n\
       Supported: 3mf, stl, obj, ply, off, amf"
    )
  })?;

  if geometries.is_empty() {
    return Err("No geometry to export".to_string());
  }

  use manifold_sys::*;

  let manifolds: Vec<Manifold> = geometries
    .iter()
    .filter_map(|geom| {
      // Prefer walking the ScadNode tree directly in Manifold
      if let Some(ref scad) = geom.scad {
        let m = materialize_scad_manifold(scad);
        if !m.is_empty() {
          return Some(m);
        }
      }
      // Fall back to converting a pre-materialized csgrs mesh
      #[cfg(feature = "csgrs")]
      {
        if let Some(ref mesh) = geom.mesh
          && !mesh.polygons.is_empty()
        {
          return csg_mesh_to_manifold(mesh).ok();
        }
      }
      None
    })
    .collect();

  if manifolds.is_empty() {
    return Err("No geometry to export".to_string());
  }

  // Union all geometries together
  let result = manifolds
    .into_iter()
    .reduce(|acc, next| {
      Manifold(unsafe {
        manifold_union(Manifold::alloc(), acc.ptr(), next.ptr())
      })
    })
    .unwrap();

  let mesh = extract_manifold_mesh(&result);

  match fmt {
    ManifoldFormat::ThreeMF => write_manifold_3mf(&mesh, path),
    ManifoldFormat::Stl => write_manifold_stl(&mesh, path),
    ManifoldFormat::Obj => write_manifold_obj(&mesh, path),
    ManifoldFormat::Ply => write_manifold_ply(&mesh, path),
    ManifoldFormat::Off => write_manifold_off(&mesh, path),
    ManifoldFormat::Amf => write_manifold_amf(&mesh, path),
  }
}

/// Backwards-compatible wrapper for 3MF-only callers.
pub fn export_manifold_3mf(
  geometries: &[CsgGeometry],
  path: &std::path::Path,
) -> Result<(), String> {
  export_manifold(geometries, "3mf", path)
}

#[cfg(all(test, feature = "csgrs"))]
mod tests {
  use super::*;
  use crate::scad_export::ScadNode;
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
      mesh: Some(cube),
      color: None,
      scad: None,
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
      mesh: Some(result),
      color: None,
      scad: None,
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
      mesh: Some(result),
      color: None,
      scad: None,
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
      mesh: Some(result),
      color: None,
      scad: None,
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
      mesh: Some(cube),
      color: None,
      scad: None,
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
      mesh: Some(cube),
      color: None,
      scad: None,
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
      mesh: Some(result),
      color: None,
      scad: None,
    };

    let dir = std::env::temp_dir().join("luacad_test");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("cube_minus_cyl.3mf");

    export_3mf(&[geom], &path).expect("export_3mf should succeed");
    assert!(path.exists());
    assert!(std::fs::metadata(&path).unwrap().len() > 0);

    let _ = std::fs::remove_file(&path);
  }

  // --- Manifold export path tests ---

  /// Helper: create a CsgGeometry with only a ScadNode (no pre-materialized mesh).
  fn geom_from_scad(scad: ScadNode) -> CsgGeometry {
    CsgGeometry {
      mesh: None,
      color: None,
      scad: Some(scad),
    }
  }

  /// Helper: export via manifold path and assert file is non-empty.
  fn assert_manifold_3mf_ok(geoms: &[CsgGeometry], name: &str) {
    let dir = std::env::temp_dir().join("luacad_test_manifold");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(format!("{name}.3mf"));
    export_manifold_3mf(geoms, &path)
      .unwrap_or_else(|e| panic!("export_manifold_3mf failed for {name}: {e}"));
    assert!(path.exists(), "{name}: file should exist");
    assert!(
      std::fs::metadata(&path).unwrap().len() > 0,
      "{name}: file should be non-empty"
    );
    let _ = std::fs::remove_file(&path);
  }

  #[test]
  fn manifold_cube_primitive() {
    let geom = geom_from_scad(ScadNode::Cube {
      w: 10.0,
      d: 10.0,
      h: 10.0,
      center: false,
    });
    assert_manifold_3mf_ok(&[geom], "manifold_cube");
  }

  #[test]
  fn manifold_sphere_primitive() {
    let geom = geom_from_scad(ScadNode::Sphere {
      r: 5.0,
      segments: 16,
    });
    assert_manifold_3mf_ok(&[geom], "manifold_sphere");
  }

  #[test]
  fn manifold_cylinder_primitive() {
    let geom = geom_from_scad(ScadNode::Cylinder {
      r1: 3.0,
      r2: 3.0,
      h: 10.0,
      segments: 32,
      center: false,
    });
    assert_manifold_3mf_ok(&[geom], "manifold_cylinder");
  }

  #[test]
  fn manifold_union_two_cubes() {
    let scad = ScadNode::Union(vec![
      ScadNode::Cube {
        w: 10.0,
        d: 10.0,
        h: 10.0,
        center: false,
      },
      ScadNode::Translate {
        x: 5.0,
        y: 5.0,
        z: 5.0,
        child: Box::new(ScadNode::Cube {
          w: 5.0,
          d: 5.0,
          h: 5.0,
          center: false,
        }),
      },
    ]);
    assert_manifold_3mf_ok(&[geom_from_scad(scad)], "manifold_union");
  }

  #[test]
  fn manifold_difference_cube_minus_cylinder() {
    let scad = ScadNode::Difference(vec![
      ScadNode::Cube {
        w: 10.0,
        d: 10.0,
        h: 10.0,
        center: true,
      },
      ScadNode::Cylinder {
        r1: 3.0,
        r2: 3.0,
        h: 20.0,
        segments: 32,
        center: true,
      },
    ]);
    assert_manifold_3mf_ok(&[geom_from_scad(scad)], "manifold_difference");
  }

  #[test]
  fn manifold_intersection() {
    let scad = ScadNode::Intersection(vec![
      ScadNode::Cube {
        w: 10.0,
        d: 10.0,
        h: 10.0,
        center: true,
      },
      ScadNode::Sphere {
        r: 7.0,
        segments: 32,
      },
    ]);
    assert_manifold_3mf_ok(&[geom_from_scad(scad)], "manifold_intersection");
  }

  #[test]
  fn manifold_translate_rotate_scale() {
    let scad = ScadNode::Scale {
      x: 2.0,
      y: 1.0,
      z: 0.5,
      child: Box::new(ScadNode::Rotate {
        x: 0.0,
        y: 0.0,
        z: 45.0,
        child: Box::new(ScadNode::Translate {
          x: 5.0,
          y: 0.0,
          z: 0.0,
          child: Box::new(ScadNode::Cube {
            w: 4.0,
            d: 4.0,
            h: 4.0,
            center: true,
          }),
        }),
      }),
    };
    assert_manifold_3mf_ok(&[geom_from_scad(scad)], "manifold_transforms");
  }

  #[test]
  fn manifold_mirror() {
    let scad = ScadNode::Mirror {
      x: 1.0,
      y: 0.0,
      z: 0.0,
      child: Box::new(ScadNode::Translate {
        x: 5.0,
        y: 0.0,
        z: 0.0,
        child: Box::new(ScadNode::Cube {
          w: 3.0,
          d: 3.0,
          h: 3.0,
          center: false,
        }),
      }),
    };
    assert_manifold_3mf_ok(&[geom_from_scad(scad)], "manifold_mirror");
  }

  #[test]
  fn manifold_hull() {
    let scad = ScadNode::Hull(Box::new(ScadNode::Union(vec![
      ScadNode::Cube {
        w: 5.0,
        d: 5.0,
        h: 5.0,
        center: false,
      },
      ScadNode::Translate {
        x: 10.0,
        y: 10.0,
        z: 10.0,
        child: Box::new(ScadNode::Sphere {
          r: 2.0,
          segments: 16,
        }),
      },
    ])));
    assert_manifold_3mf_ok(&[geom_from_scad(scad)], "manifold_hull");
  }

  #[test]
  fn manifold_minkowski_sum() {
    let scad = ScadNode::Minkowski(vec![
      ScadNode::Cube {
        w: 10.0,
        d: 10.0,
        h: 2.0,
        center: false,
      },
      ScadNode::Sphere {
        r: 1.0,
        segments: 8,
      },
    ]);
    assert_manifold_3mf_ok(&[geom_from_scad(scad)], "manifold_minkowski");
  }

  #[test]
  fn manifold_gear_model() {
    // Reproduces the gear example: cylinder + rotated translated cubes
    let num_teeth = 8;
    let height: f32 = 5.0;
    let radius: f32 = 10.0;
    let tooth_length = radius * 0.3;
    let tooth_width = radius * 0.2;

    let mut children = vec![ScadNode::Cylinder {
      r1: radius * 0.7,
      r2: radius * 0.7,
      h: height,
      segments: 32,
      center: false,
    }];

    for i in 1..=num_teeth {
      let angle = i as f32 * (360.0 / num_teeth as f32);
      children.push(ScadNode::Rotate {
        x: 0.0,
        y: 0.0,
        z: angle,
        child: Box::new(ScadNode::Translate {
          x: radius - (tooth_length * 1.2),
          y: -tooth_width / 2.0,
          z: 0.0,
          child: Box::new(ScadNode::Cube {
            w: tooth_length * 1.2,
            d: tooth_width,
            h: height,
            center: false,
          }),
        }),
      });
    }

    let scad = ScadNode::Union(children);
    assert_manifold_3mf_ok(&[geom_from_scad(scad)], "manifold_gear");
  }

  #[test]
  fn manifold_box_model() {
    // Reproduces the box example: outer cube minus inner cube
    let scad = ScadNode::Difference(vec![
      ScadNode::Cube {
        w: 30.0,
        d: 20.0,
        h: 15.0,
        center: false,
      },
      ScadNode::Translate {
        x: 2.0,
        y: 2.0,
        z: 2.0,
        child: Box::new(ScadNode::Cube {
          w: 26.0,
          d: 16.0,
          h: 15.0,
          center: false,
        }),
      },
    ]);
    assert_manifold_3mf_ok(&[geom_from_scad(scad)], "manifold_box");
  }

  #[test]
  fn manifold_multiple_geometries_unioned() {
    let geoms = vec![
      geom_from_scad(ScadNode::Cube {
        w: 5.0,
        d: 5.0,
        h: 5.0,
        center: false,
      }),
      geom_from_scad(ScadNode::Translate {
        x: 10.0,
        y: 0.0,
        z: 0.0,
        child: Box::new(ScadNode::Sphere {
          r: 3.0,
          segments: 16,
        }),
      }),
    ];
    assert_manifold_3mf_ok(&geoms, "manifold_multi_geom");
  }

  #[test]
  fn manifold_fallback_to_csgrs_mesh() {
    // Geometry with only a pre-materialized mesh and no ScadNode
    let cube = CsgMesh::<()>::cuboid(5.0, 5.0, 5.0, None);
    let geom = CsgGeometry {
      mesh: Some(cube),
      color: None,
      scad: None,
    };
    assert_manifold_3mf_ok(&[geom], "manifold_fallback_mesh");
  }

  #[test]
  fn manifold_empty_geometries_returns_error() {
    let result = export_manifold_3mf(
      &[],
      &std::path::PathBuf::from("/tmp/should_not_exist.3mf"),
    );
    assert!(result.is_err());
  }
}

/// Manifold-only tests — these run without the `csgrs` feature.
#[cfg(test)]
mod manifold_tests {
  use super::*;
  use crate::scad_export::ScadNode;

  #[test]
  fn minkowski_offset_operand() {
    // Regression: the face-sweep Minkowski decomposition is only valid
    // when the swept operand contains the origin. With a far-translated
    // sphere the result must be the translated sum — not a blend that
    // also spans the first operand's original location.
    let l_shape = ScadNode::Difference(vec![
      ScadNode::Cube {
        w: 10.0,
        d: 10.0,
        h: 10.0,
        center: false,
      },
      ScadNode::Translate {
        x: 5.0,
        y: 5.0,
        z: -1.0,
        child: Box::new(ScadNode::Cube {
          w: 10.0,
          d: 10.0,
          h: 15.0,
          center: false,
        }),
      },
    ]);
    let scad = ScadNode::Minkowski(vec![
      l_shape,
      ScadNode::Translate {
        x: 30.0,
        y: 0.0,
        z: 0.0,
        child: Box::new(ScadNode::Sphere {
          r: 2.0,
          segments: 16,
        }),
      },
    ]);
    let mesh = extract_manifold_mesh(&materialize_scad_manifold(&scad));
    assert!(!mesh.vertices.is_empty());
    let (min_x, max_x) = mesh
      .vertices
      .iter()
      .fold((f32::INFINITY, f32::NEG_INFINITY), |(lo, hi), v| {
        (lo.min(v[0]), hi.max(v[0]))
      });
    // L-shape spans x ∈ [0, 10], sphere spans x ∈ [28, 32],
    // so the Minkowski sum must span x ∈ [28, 42].
    assert!(
      (min_x - 28.0).abs() < 0.01,
      "min x should be ~28, got {min_x}"
    );
    assert!(
      (max_x - 42.0).abs() < 0.01,
      "max x should be ~42, got {max_x}"
    );
  }
}

/// Tests for the 2D → 3D part of the Manifold path. Unlike the module
/// above these don't touch csgrs, so they run with default features.
#[cfg(test)]
mod cross_section_tests {
  use super::*;
  use crate::scad_export::ScadNode;

  fn square(w: f32, h: f32, center: bool) -> ScadNode {
    ScadNode::Square { w, h, center }
  }

  fn extrude(child: ScadNode, height: f32) -> ScadNode {
    ScadNode::LinearExtrude {
      height,
      center: false,
      twist: 0.0,
      slices: 0,
      scale: 1.0,
      child: Box::new(child),
    }
  }

  fn assert_close(actual: f64, expected: f64, tol: f64, what: &str) {
    assert!(
      (actual - expected).abs() < tol,
      "{what}: expected ~{expected}, got {actual}"
    );
  }

  #[test]
  fn linear_extrude_square() {
    let m = materialize_scad_manifold(&extrude(square(10.0, 4.0, false), 3.0));
    assert!(m.num_tri() > 0, "extruded square should produce triangles");
    assert_close(m.volume(), 120.0, 1e-6, "volume");
    let (min, max) = m.bounding_box();
    assert_eq!(min, [0.0, 0.0, 0.0]);
    assert_eq!(max, [10.0, 4.0, 3.0]);
  }

  #[test]
  fn linear_extrude_centered_straddles_z_zero() {
    let scad = ScadNode::LinearExtrude {
      height: 8.0,
      center: true,
      twist: 0.0,
      slices: 0,
      scale: 1.0,
      child: Box::new(square(2.0, 2.0, false)),
    };
    let (min, max) = materialize_scad_manifold(&scad).bounding_box();
    assert_close(min[2] as f64, -4.0, 1e-5, "min z");
    assert_close(max[2] as f64, 4.0, 1e-5, "max z");
  }

  #[test]
  fn linear_extrude_scale_makes_frustum() {
    let scad = ScadNode::LinearExtrude {
      height: 3.0,
      center: false,
      twist: 0.0,
      slices: 0,
      scale: 0.5,
      child: Box::new(square(10.0, 4.0, false)),
    };
    // Frustum volume = h/3 * (A_bottom + A_top + sqrt(A_bottom * A_top))
    let m = materialize_scad_manifold(&scad);
    assert_close(m.volume(), 70.0, 1e-4, "frustum volume");
  }

  #[test]
  fn linear_extrude_twists_clockwise_like_openscad() {
    let scad = ScadNode::LinearExtrude {
      height: 5.0,
      center: false,
      twist: 90.0,
      slices: 8,
      scale: 1.0,
      child: Box::new(square(2.0, 2.0, false)),
    };
    let (min, max) = materialize_scad_manifold(&scad).bounding_box();
    // Seen from +Z the top face rotates from x ∈ [0, 2], y ∈ [0, 2]
    // into x ∈ [0, 2], y ∈ [-2, 0]. A counter-clockwise twist would
    // instead push x negative and leave y positive.
    assert_close(min[1] as f64, -2.0, 1e-4, "min y");
    assert_close(min[0] as f64, 0.0, 1e-4, "min x");
    assert!(max[0] > 2.0, "swept corners should exceed x = 2");
  }

  #[test]
  fn rotate_extrude_square_makes_torus() {
    let scad = ScadNode::RotateExtrude {
      angle: 360.0,
      segments: 128,
      child: Box::new(ScadNode::Translate {
        x: 5.0,
        y: 0.0,
        z: 0.0,
        child: Box::new(square(1.0, 1.0, false)),
      }),
    };
    let m = materialize_scad_manifold(&scad);
    assert!(m.num_tri() > 0, "revolved square should produce triangles");
    // Pappus: V = 2π · centroid radius · area = 2π · 5.5 · 1
    assert_close(m.volume(), 2.0 * std::f64::consts::PI * 5.5, 0.02, "volume");
    let (min, max) = m.bounding_box();
    assert_close(min[0] as f64, -6.0, 0.01, "min x");
    assert_close(max[2] as f64, 1.0, 1e-5, "max z");
  }

  #[test]
  fn polygon_extrude() {
    let scad = extrude(
      ScadNode::Polygon {
        points: vec![[0.0, 0.0], [4.0, 0.0], [4.0, 3.0]],
      },
      2.0,
    );
    let m = materialize_scad_manifold(&scad);
    // Right triangle of area 6, extruded 2 high
    assert_close(m.volume(), 12.0, 1e-5, "volume");
  }

  #[test]
  fn degenerate_polygon_yields_empty() {
    let scad = extrude(
      ScadNode::Polygon {
        points: vec![[0.0, 0.0], [1.0, 0.0]],
      },
      2.0,
    );
    assert_eq!(materialize_scad_manifold(&scad).num_tri(), 0);
  }

  #[test]
  fn hull_of_circles_extrudes_to_rounded_box() {
    // The examples/rounded_rectangle.lua shape: four corner circles,
    // hulled into a stadium outline, then extruded.
    let (w, h, r) = (20.0f32, 10.0f32, 2.0f32);
    let corner = |x: f32, y: f32| ScadNode::Translate {
      x,
      y,
      z: 0.0,
      child: Box::new(ScadNode::Circle { r, segments: 32 }),
    };
    let scad = extrude(
      ScadNode::Hull(Box::new(ScadNode::Union(vec![
        corner(r, r),
        corner(w - r, r),
        corner(r, h - r),
        corner(w - r, h - r),
      ]))),
      5.0,
    );
    let m = materialize_scad_manifold(&scad);
    assert!(m.num_tri() > 0, "hulled sketch should produce triangles");
    let (min, max) = m.bounding_box();
    assert_eq!(min, [0.0, 0.0, 0.0]);
    assert_close(max[0] as f64, w as f64, 1e-4, "max x");
    assert_close(max[1] as f64, h as f64, 1e-4, "max y");
    assert_close(max[2] as f64, 5.0, 1e-5, "max z");
  }

  #[test]
  fn difference_of_sketches() {
    let scad = extrude(
      ScadNode::Difference(vec![
        square(10.0, 10.0, false),
        ScadNode::Translate {
          x: 2.0,
          y: 2.0,
          z: 0.0,
          child: Box::new(square(6.0, 6.0, false)),
        },
      ]),
      1.0,
    );
    // 100 - 36, extruded 1 high
    assert_close(
      materialize_scad_manifold(&scad).volume(),
      64.0,
      1e-5,
      "volume",
    );
  }

  #[test]
  fn offset_grows_the_outline() {
    let scad = extrude(
      ScadNode::Offset {
        delta: None,
        r: Some(2.0),
        chamfer: false,
        child: Box::new(square(10.0, 10.0, false)),
      },
      1.0,
    );
    let (min, max) = materialize_scad_manifold(&scad).bounding_box();
    assert_close(min[0] as f64, -2.0, 0.01, "min x");
    assert_close(max[0] as f64, 12.0, 0.01, "max x");
  }

  #[test]
  fn projection_flattens_a_solid() {
    let scad = extrude(
      ScadNode::Projection {
        cut: false,
        child: Box::new(ScadNode::Sphere {
          r: 5.0,
          segments: 64,
        }),
      },
      1.0,
    );
    let m = materialize_scad_manifold(&scad);
    assert!(m.num_tri() > 0, "projection should produce triangles");
    let (min, max) = m.bounding_box();
    assert_close(min[0] as f64, -5.0, 0.05, "min x");
    assert_close(max[0] as f64, 5.0, 0.05, "max x");
    assert_close(max[2] as f64, 1.0, 1e-5, "max z");
  }

  #[test]
  fn unsupported_sketch_yields_empty_solid() {
    let scad = extrude(
      ScadNode::Text {
        text: "hi".to_string(),
        size: 10.0,
        font: String::new(),
        halign: "left".to_string(),
        valign: "baseline".to_string(),
      },
      1.0,
    );
    assert_eq!(materialize_scad_manifold(&scad).num_tri(), 0);
  }
}
