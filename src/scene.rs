use three_d::*;

use crate::app::AppState;
use crate::geometry::csg_to_cpu_mesh;

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
