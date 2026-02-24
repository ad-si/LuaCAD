use three_d::*;

use csgrs::mesh::Mesh as CsgMesh;
use csgrs::traits::CSG;

use crate::app::AppState;
use luacad::geometry::CsgGeometry;

/// Convert a csgrs mesh to a three-d CpuMesh.
/// Applies CAD Z-up → GL Y-up coordinate swap: (x,y,z) → (y,z,x)
fn csg_to_cpu_mesh(csg: &CsgMesh<()>) -> CpuMesh {
  let tri_mesh = csg.triangulate();

  let mut positions: Vec<Vec3> = Vec::new();
  let mut normals: Vec<Vec3> = Vec::new();
  let mut indices: Vec<u32> = Vec::new();

  for polygon in &tri_mesh.polygons {
    let base_idx = positions.len() as u32;
    for vertex in &polygon.vertices {
      let p = &vertex.pos;
      let n = &vertex.normal;
      // CAD (x,y,z) → GL (y,z,x)
      positions.push(vec3(p.y, p.z, p.x));
      normals.push(vec3(n.y, n.z, n.x));
    }
    // Fan-triangulate polygons with more than 3 vertices
    for i in 1..polygon.vertices.len().saturating_sub(1) {
      indices.push(base_idx);
      indices.push(base_idx + i as u32);
      indices.push(base_idx + i as u32 + 1);
    }
  }

  CpuMesh {
    positions: Positions::F32(positions),
    indices: Indices::U32(indices),
    normals: Some(normals),
    ..Default::default()
  }
}

/// Build 3D mesh objects from CSG geometry.
/// Coordinate transform (CAD Z-up → GL Y-up) is done inside csg_to_cpu_mesh.
pub fn build_scene(
  context: &Context,
  app: &AppState,
) -> Vec<Gm<Mesh, PhysicalMaterial>> {
  app
    .geometries
    .iter()
    .filter(|geom| !geom.mesh.polygons.is_empty())
    .map(|geom| {
      let cpu_mesh = csg_to_cpu_mesh(&geom.mesh);
      let (r, g, b) = geom
        .color
        .map(|c| {
          (
            (c[0] * 255.0) as u8,
            (c[1] * 255.0) as u8,
            (c[2] * 255.0) as u8,
          )
        })
        .unwrap_or((150, 150, 255));
      Gm::new(
        Mesh::new(context, &cpu_mesh),
        PhysicalMaterial::new_opaque(
          context,
          &CpuMaterial {
            albedo: Srgba { r, g, b, a: 255 },
            metallic: 0.0,
            roughness: 0.7,
            ..Default::default()
          },
        ),
      )
    })
    .collect()
}

/// Compute the camera distance needed to fit all geometries in view.
/// Returns `None` if there are no geometries.
pub fn compute_fit_distance(
  geometries: &[CsgGeometry],
  orthogonal: bool,
) -> Option<f32> {
  if geometries.is_empty() {
    return None;
  }

  let mut max_extent: f32 = 0.0;
  for geom in geometries {
    if geom.mesh.polygons.is_empty() {
      continue;
    }
    let bb = geom.mesh.bounding_box();
    // Check all 8 corners, converting CAD (x,y,z) → GL (y,z,x)
    for &cx in &[bb.mins.x, bb.maxs.x] {
      for &cy in &[bb.mins.y, bb.maxs.y] {
        for &cz in &[bb.mins.z, bb.maxs.z] {
          let gl = vec3(cy, cz, cx);
          max_extent = max_extent.max(gl.magnitude());
        }
      }
    }
  }

  if max_extent < 1e-6 {
    return None;
  }

  let padding = 1.3;
  if orthogonal {
    // Orthographic height is 2.0, three-d multiplies by distance
    Some(max_extent * padding)
  } else {
    // Perspective FOV = 45°, half-angle = 22.5°
    Some(max_extent * padding / 22.5_f32.to_radians().tan())
  }
}

/// Compute camera position from azimuth/elevation/distance.
/// Returns (position, target, up) in Y-up coordinate system.
pub fn compute_camera_vectors(app: &AppState) -> (Vec3, Vec3, Vec3) {
  let az = app.camera_azimuth.to_radians();
  let el = app.camera_elevation.to_radians();
  let d = app.camera_distance;

  let x = d * el.cos() * az.sin();
  let y = d * el.sin();
  let z = d * el.cos() * az.cos();

  let position = vec3(x, y, z);
  let target = vec3(0.0, 0.0, 0.0);
  let up = vec3(0.0, 1.0, 0.0);

  (position, target, up)
}

pub fn build_camera(viewport: Viewport, app: &AppState) -> Camera {
  let (pos, target, up) = compute_camera_vectors(app);
  if app.orthogonal_view {
    // three-d multiplies height by camera-to-target distance internally,
    // so use a fixed height. Negative z_near prevents any front clipping.
    Camera::new_orthographic(
      viewport,
      pos,
      target,
      up,
      2.0,
      -100.0 * app.camera_distance,
      100.0 * app.camera_distance,
    )
  } else {
    Camera::new_perspective(
      viewport,
      pos,
      target,
      up,
      degrees(45.0),
      0.1 * app.camera_distance,
      100.0 * app.camera_distance,
    )
  }
}
