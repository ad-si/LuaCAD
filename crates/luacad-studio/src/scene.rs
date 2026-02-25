use three_d::*;

use crate::app::AppState;
use crate::csg_tree::CsgGroup;
use csgrs::traits::CSG;
use luacad::geometry::CsgGeometry;
use opencsg_sys::OcsgPrimitive;
use std::ffi::c_void;

/// Data passed to the OpenCSG render callback for each leaf primitive.
struct LeafRenderData {
  vertices: *const [f32; 3],
  vertex_count: usize,
  transform: [f32; 16],
  /// Pre-computed per-vertex normals (one per face, repeated 3x per triangle).
  normals: Vec<[f32; 3]>,
}

/// OpenCSG render callback: draws the leaf's triangulated geometry.
unsafe extern "C" fn render_leaf_callback(user_data: *mut c_void) {
  let data = unsafe { &*(user_data as *const LeafRenderData) };
  let verts =
    unsafe { std::slice::from_raw_parts(data.vertices, data.vertex_count) };

  unsafe {
    // Apply the leaf's transform (CAD → world, column-major)
    gl_PushMatrix();
    gl_MultMatrixf(data.transform.as_ptr());

    // Draw triangles via vertex arrays
    gl_EnableClientState(GL_VERTEX_ARRAY);
    gl_VertexPointer(3, GL_FLOAT, 0, verts.as_ptr() as *const c_void);
    gl_DrawArrays(GL_TRIANGLES, 0, data.vertex_count as i32);
    gl_DisableClientState(GL_VERTEX_ARRAY);

    gl_PopMatrix();
  }
}

/// Render the full CSG scene using OpenCSG.
///
/// This performs OpenCSG's z-buffer CSG for each group, then a shading pass
/// with fixed-function lighting and `GL_EQUAL` depth test.
pub fn render_opencsg_scene(
  groups: &[CsgGroup],
  projection: &[f32; 16],
  view: &[f32; 16],
) {
  unsafe {
    // Ensure we're using the fixed-function pipeline (no shader program active).
    // egui_glow leaves a shader program bound after rendering which would
    // intercept our legacy GL calls.
    gl_UseProgram(0);

    // Unbind any VAO left by glow/egui so that legacy client-side vertex
    // arrays (glVertexPointer, glNormalPointer) work correctly.
    // On GL 3.0+ contexts, a non-default VAO ignores client pointers.
    gl_BindVertexArray(0);

    // Set up legacy GL matrices from camera
    gl_MatrixMode(GL_PROJECTION);
    gl_LoadMatrixf(projection.as_ptr());

    // Set up fixed-function lighting
    gl_Enable(GL_LIGHTING);
    gl_Enable(GL_LIGHT0);
    gl_Enable(GL_LIGHT1);
    gl_Enable(GL_LIGHT2);
    gl_Enable(GL_NORMALIZE);
    gl_Enable(GL_COLOR_MATERIAL);
    gl_ColorMaterial(GL_FRONT_AND_BACK, GL_AMBIENT_AND_DIFFUSE);

    // Ambient light (35%)
    let ambient: [f32; 4] = [0.35, 0.35, 0.35, 1.0];
    gl_LightModelfv(GL_LIGHT_MODEL_AMBIENT, ambient.as_ptr());

    // Two-sided lighting: subtracted primitives expose inner surfaces whose
    // normals face away from the viewer. This ensures they're still lit.
    gl_LightModeli(GL_LIGHT_MODEL_TWO_SIDE, 1);

    let no_amb: [f32; 4] = [0.0, 0.0, 0.0, 1.0];

    // Set light positions with identity modelview so they remain fixed in
    // world space and don't move when the camera orbits.
    gl_MatrixMode(GL_MODELVIEW);
    gl_LoadIdentity();

    // Key light (90%, top-right-front) with specular
    let light0_dir: [f32; 4] = [1.0, 1.0, 0.5, 0.0]; // directional
    let light0_diff: [f32; 4] = [0.9, 0.9, 0.9, 1.0];
    let light0_spec: [f32; 4] = [0.6, 0.6, 0.6, 1.0];
    gl_Lightfv(GL_LIGHT0, GL_POSITION, light0_dir.as_ptr());
    gl_Lightfv(GL_LIGHT0, GL_DIFFUSE, light0_diff.as_ptr());
    gl_Lightfv(GL_LIGHT0, GL_SPECULAR, light0_spec.as_ptr());
    gl_Lightfv(GL_LIGHT0, GL_AMBIENT, no_amb.as_ptr());

    // Fill light (55%, front-left, slightly above)
    let light1_dir: [f32; 4] = [-1.0, 0.3, 0.5, 0.0];
    let light1_diff: [f32; 4] = [0.55, 0.55, 0.55, 1.0];
    gl_Lightfv(GL_LIGHT1, GL_POSITION, light1_dir.as_ptr());
    gl_Lightfv(GL_LIGHT1, GL_DIFFUSE, light1_diff.as_ptr());
    gl_Lightfv(GL_LIGHT1, GL_AMBIENT, no_amb.as_ptr());

    // Bottom light (40%, from below)
    let light2_dir: [f32; 4] = [0.0, -1.0, 0.0, 0.0];
    let light2_diff: [f32; 4] = [0.4, 0.4, 0.4, 1.0];
    gl_Lightfv(GL_LIGHT2, GL_POSITION, light2_dir.as_ptr());
    gl_Lightfv(GL_LIGHT2, GL_DIFFUSE, light2_diff.as_ptr());
    gl_Lightfv(GL_LIGHT2, GL_AMBIENT, no_amb.as_ptr());

    // Now load the actual view matrix for geometry rendering
    gl_LoadMatrixf(view.as_ptr());

    // Material specular properties (subtle highlight, medium shininess)
    let mat_spec: [f32; 4] = [0.4, 0.4, 0.4, 1.0];
    let mat_shin: f32 = 25.0;
    gl_Materialfv(GL_FRONT_AND_BACK, GL_SPECULAR, mat_spec.as_ptr());
    gl_Materialf(GL_FRONT_AND_BACK, GL_SHININESS, mat_shin);

    // Disable lighting for the OpenCSG depth pass (it only cares about geometry)
    gl_Disable(GL_LIGHTING);
  }

  for group in groups {
    render_csg_group(group, projection, view);
  }

  unsafe {
    // Clean up lighting state
    gl_Disable(GL_LIGHTING);
    gl_Disable(GL_LIGHT0);
    gl_Disable(GL_LIGHT1);
    gl_Disable(GL_LIGHT2);
    gl_Disable(GL_NORMALIZE);
    gl_Disable(GL_COLOR_MATERIAL);
  }
}

/// Render a single CSG group: OpenCSG depth pass + shading pass.
fn render_csg_group(
  group: &CsgGroup,
  projection: &[f32; 16],
  view: &[f32; 16],
) {
  // Filter to non-empty leaves and collect render data + colors together.
  let active_leaves: Vec<_> = group
    .primitives
    .iter()
    .filter(|leaf| !leaf.vertices.is_empty())
    .collect();

  if active_leaves.is_empty() {
    return;
  }

  let mut render_datas: Vec<LeafRenderData> =
    Vec::with_capacity(active_leaves.len());
  let mut ocsg_prims: Vec<*mut OcsgPrimitive> =
    Vec::with_capacity(active_leaves.len());

  for leaf in &active_leaves {
    // Pre-compute per-face normals (one normal repeated for each vertex of a triangle).
    let normals = compute_face_normals(&leaf.vertices);
    render_datas.push(LeafRenderData {
      vertices: leaf.vertices.as_ptr(),
      vertex_count: leaf.vertices.len(),
      transform: cad_to_gl_transform(&leaf.transform),
      normals,
    });
  }

  // Create OpenCSG primitives with callbacks pointing to our render data.
  for (i, leaf) in active_leaves.iter().enumerate() {
    let prim = unsafe {
      opencsg_sys::primitive_new(
        leaf.operation,
        leaf.convexity,
        render_leaf_callback,
        &render_datas[i] as *const LeafRenderData as *mut c_void,
      )
    };
    ocsg_prims.push(prim);
  }

  if ocsg_prims.is_empty() {
    return;
  }

  // Check for debug bypass: skip OpenCSG and render directly
  let skip_csg = std::env::var("LUACAD_DEBUG_NO_CSG").is_ok();

  if !skip_csg {
    // --- OpenCSG depth pass ---
    unsafe {
      opencsg_sys::render(&mut ocsg_prims);
    }
  }

  // --- Shading pass ---
  unsafe {
    gl_UseProgram(0);
    gl_MatrixMode(GL_PROJECTION);
    gl_LoadMatrixf(projection.as_ptr());
    gl_MatrixMode(GL_MODELVIEW);
    gl_LoadMatrixf(view.as_ptr());
    gl_DepthFunc(if skip_csg { GL_LEQUAL } else { GL_EQUAL });
    gl_Enable(GL_LIGHTING);
    gl_Enable(GL_LIGHT0);
    gl_Enable(GL_LIGHT1);
    gl_Enable(GL_LIGHT2);
    gl_Enable(GL_NORMALIZE);
    gl_Enable(GL_COLOR_MATERIAL);
    gl_ColorMaterial(GL_FRONT_AND_BACK, GL_AMBIENT_AND_DIFFUSE);
    gl_ShadeModel(GL_SMOOTH);

    for (i, leaf) in active_leaves.iter().enumerate() {
      gl_Color3f(leaf.color[0], leaf.color[1], leaf.color[2]);

      let data = &render_datas[i];

      gl_PushMatrix();
      gl_MultMatrixf(data.transform.as_ptr());

      // Use vertex arrays for both positions and normals so the depth
      // values match the OpenCSG depth pass exactly.
      gl_EnableClientState(GL_VERTEX_ARRAY);
      gl_EnableClientState(GL_NORMAL_ARRAY);
      gl_VertexPointer(3, GL_FLOAT, 0, data.vertices as *const c_void);
      gl_NormalPointer(GL_FLOAT, 0, data.normals.as_ptr() as *const c_void);
      gl_DrawArrays(GL_TRIANGLES, 0, data.vertex_count as i32);
      gl_DisableClientState(GL_NORMAL_ARRAY);
      gl_DisableClientState(GL_VERTEX_ARRAY);

      gl_PopMatrix();
    }

    gl_DepthFunc(GL_LEQUAL);
    gl_Disable(GL_LIGHTING);
  }

  // Free OpenCSG primitives
  for prim in ocsg_prims {
    unsafe {
      opencsg_sys::primitive_free(prim);
    }
  }
}

/// Compute per-face normals for triangle vertices. Returns one normal per vertex
/// (each triangle's 3 vertices share the same face normal).
fn compute_face_normals(verts: &[[f32; 3]]) -> Vec<[f32; 3]> {
  let mut normals = Vec::with_capacity(verts.len());
  for tri in verts.chunks_exact(3) {
    let a = tri[0];
    let b = tri[1];
    let c = tri[2];
    let ab = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
    let ac = [c[0] - a[0], c[1] - a[1], c[2] - a[2]];
    let n = [
      ab[1] * ac[2] - ab[2] * ac[1],
      ab[2] * ac[0] - ab[0] * ac[2],
      ab[0] * ac[1] - ab[1] * ac[0],
    ];
    normals.push(n);
    normals.push(n);
    normals.push(n);
  }
  normals
}

/// Draw 3D axes at the origin using raw GL.
/// CAD convention: Red=X, Green=Y, Blue=Z.
/// Mapping: CAD (x,y,z) → GL (y,z,x).
pub fn render_axes() {
  let len = 5.0_f32;
  unsafe {
    gl_Disable(GL_LIGHTING);
    gl_LineWidth(2.0);
    gl_Begin(GL_LINES);

    // CAD X axis (red) → GL Z
    gl_Color3f(1.0, 0.0, 0.0);
    gl_Vertex3f(0.0, 0.0, 0.0);
    gl_Vertex3f(0.0, 0.0, len);

    // CAD Y axis (green) → GL X
    gl_Color3f(0.0, 1.0, 0.0);
    gl_Vertex3f(0.0, 0.0, 0.0);
    gl_Vertex3f(len, 0.0, 0.0);

    // CAD Z axis (blue) → GL Y
    gl_Color3f(0.3, 0.3, 1.0);
    gl_Vertex3f(0.0, 0.0, 0.0);
    gl_Vertex3f(0.0, len, 0.0);

    gl_End();
    gl_LineWidth(1.0);

    // Debug: render a test triangle via immediate mode (yellow)
    if std::env::var("LUACAD_DEBUG_TRIANGLE").is_ok() {
      gl_Color3f(1.0, 1.0, 0.0);
      gl_Begin(GL_TRIANGLES);
      gl_Vertex3f(0.0, 0.0, 0.0);
      gl_Vertex3f(2.0, 0.0, 0.0);
      gl_Vertex3f(1.0, 2.0, 0.0);
      gl_End();

      // Debug: render a test triangle via vertex arrays (cyan)
      let verts: [[f32; 3]; 3] = [
        [0.0, 0.0, 0.0],
        [0.0, 0.0, 2.0],
        [0.0, 2.0, 1.0],
      ];
      gl_BindVertexArray(0);
      gl_Color3f(0.0, 1.0, 1.0);
      gl_EnableClientState(GL_VERTEX_ARRAY);
      gl_VertexPointer(3, GL_FLOAT, 0, verts.as_ptr() as *const c_void);
      gl_DrawArrays(GL_TRIANGLES, 0, 3);
      gl_DisableClientState(GL_VERTEX_ARRAY);
    }
  }
}

/// Convert a CAD-space column-major transform to GL-space.
/// CAD (x,y,z) → GL (y,z,x). This permutes both the rows and columns
/// of the transform matrix.
fn cad_to_gl_transform(m: &[f32; 16]) -> [f32; 16] {
  // The coordinate swap is: GL_x = CAD_y, GL_y = CAD_z, GL_z = CAD_x
  // This is a permutation P where:
  //   P = | 0 1 0 0 |
  //       | 0 0 1 0 |
  //       | 1 0 0 0 |
  //       | 0 0 0 1 |
  // We need P * M * P^-1 (where P^-1 = P^T for permutation matrices)
  // P^T = | 0 0 1 0 |
  //       | 1 0 0 0 |
  //       | 0 1 0 0 |
  //       | 0 0 0 1 |

  // Column-major: m[col*4 + row]
  // Extract as row-major for clarity, apply P*M*P^T, convert back.

  // Helper to read m as column-major: element at (row, col)
  let at = |r: usize, c: usize| m[c * 4 + r];

  // Permutation indices: CAD x=0, y=1, z=2 → GL 2, 0, 1
  // So GL row/col i corresponds to CAD row/col perm[i]
  let perm = [1usize, 2, 0]; // GL_x ← CAD_y, GL_y ← CAD_z, GL_z ← CAD_x

  let mut out = [0.0f32; 16];
  for gr in 0..3 {
    for gc in 0..3 {
      out[gc * 4 + gr] = at(perm[gr], perm[gc]);
    }
    // Translation column: row gr = CAD row perm[gr], col 3
    out[3 * 4 + gr] = at(perm[gr], 3);
  }
  // Bottom row: (row 3, col gc) = at(3, perm[gc])
  for gc in 0..3 {
    out[gc * 4 + 3] = at(3, perm[gc]);
  }
  // (3,3) element
  out[3 * 4 + 3] = at(3, 3);

  out
}

/// Compute the camera distance needed to fit all geometries in view.
pub fn compute_fit_distance(
  geometries: &[CsgGeometry],
  orthogonal: bool,
) -> Option<f32> {
  if geometries.is_empty() {
    return None;
  }

  let mut max_extent: f32 = 0.0;
  for geom in geometries {
    let mesh = match geom.mesh.as_ref() {
      Some(m) if !m.polygons.is_empty() => m,
      _ => continue,
    };
    let bb = mesh.bounding_box();
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
    Some(max_extent * padding)
  } else {
    Some(max_extent * padding / 22.5_f32.to_radians().tan())
  }
}

/// Compute camera position from azimuth/elevation/distance.
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

/// Extract projection matrix as column-major f32 array from three-d Camera.
pub fn camera_projection_matrix(camera: &Camera) -> [f32; 16] {
  let m = camera.projection();
  // cgmath Matrix4<f32> is column-major, same memory layout as [f32; 16]
  unsafe { std::mem::transmute(m) }
}

/// Extract view matrix as column-major f32 array from three-d Camera.
pub fn camera_view_matrix(camera: &Camera) -> [f32; 16] {
  let m = camera.view();
  unsafe { std::mem::transmute(m) }
}

// --- Raw OpenGL function bindings via system libraries ---
// We need these because OpenCSG uses legacy GL, and we need to interop
// with the same GL context. glow only provides core profile functions.

#[cfg(target_os = "macos")]
#[link(name = "OpenGL", kind = "framework")]
unsafe extern "C" {
  #[link_name = "glMatrixMode"]
  fn gl_MatrixMode(mode: u32);
  #[link_name = "glLoadMatrixf"]
  fn gl_LoadMatrixf(m: *const f32);
  #[link_name = "glLoadIdentity"]
  fn gl_LoadIdentity();
  #[link_name = "glPushMatrix"]
  fn gl_PushMatrix();
  #[link_name = "glPopMatrix"]
  fn gl_PopMatrix();
  #[link_name = "glMultMatrixf"]
  fn gl_MultMatrixf(m: *const f32);
  #[link_name = "glEnable"]
  fn gl_Enable(cap: u32);
  #[link_name = "glDisable"]
  fn gl_Disable(cap: u32);
  #[link_name = "glLightfv"]
  fn gl_Lightfv(light: u32, pname: u32, params: *const f32);
  #[link_name = "glLightModelfv"]
  fn gl_LightModelfv(pname: u32, params: *const f32);
  #[link_name = "glLightModeli"]
  fn gl_LightModeli(pname: u32, param: i32);
  #[link_name = "glColorMaterial"]
  fn gl_ColorMaterial(face: u32, mode: u32);
  #[link_name = "glShadeModel"]
  fn gl_ShadeModel(mode: u32);
  #[link_name = "glDepthFunc"]
  fn gl_DepthFunc(func: u32);
  #[link_name = "glBegin"]
  fn gl_Begin(mode: u32);
  #[link_name = "glEnd"]
  fn gl_End();
  #[link_name = "glVertex3f"]
  fn gl_Vertex3f(x: f32, y: f32, z: f32);
  #[link_name = "glColor3f"]
  fn gl_Color3f(r: f32, g: f32, b: f32);
  #[link_name = "glLineWidth"]
  fn gl_LineWidth(width: f32);
  #[link_name = "glEnableClientState"]
  fn gl_EnableClientState(array: u32);
  #[link_name = "glDisableClientState"]
  fn gl_DisableClientState(array: u32);
  #[link_name = "glVertexPointer"]
  fn gl_VertexPointer(
    size: i32,
    type_: u32,
    stride: i32,
    pointer: *const c_void,
  );
  #[link_name = "glNormalPointer"]
  fn gl_NormalPointer(type_: u32, stride: i32, pointer: *const c_void);
  #[link_name = "glDrawArrays"]
  fn gl_DrawArrays(mode: u32, first: i32, count: i32);
  #[link_name = "glClear"]
  fn gl_Clear(mask: u32);
  #[link_name = "glClearColor"]
  fn gl_ClearColor(r: f32, g: f32, b: f32, a: f32);
  #[link_name = "glClearDepth"]
  fn gl_ClearDepth(depth: f64);
  #[link_name = "glClearStencil"]
  fn gl_ClearStencil(s: i32);
  #[link_name = "glViewport"]
  fn gl_Viewport(x: i32, y: i32, width: i32, height: i32);
  #[link_name = "glMaterialfv"]
  fn gl_Materialfv(face: u32, pname: u32, params: *const f32);
  #[link_name = "glMaterialf"]
  fn gl_Materialf(face: u32, pname: u32, param: f32);
  #[link_name = "glUseProgram"]
  fn gl_UseProgram(program: u32);
}

// macOS uses GL 2.1 Legacy which has no VAOs; provide a no-op.
#[cfg(target_os = "macos")]
#[allow(non_snake_case)]
unsafe fn gl_BindVertexArray(_array: u32) {}

#[cfg(target_os = "linux")]
#[link(name = "GL")]
unsafe extern "C" {
  #[link_name = "glMatrixMode"]
  fn gl_MatrixMode(mode: u32);
  #[link_name = "glLoadMatrixf"]
  fn gl_LoadMatrixf(m: *const f32);
  #[link_name = "glLoadIdentity"]
  fn gl_LoadIdentity();
  #[link_name = "glPushMatrix"]
  fn gl_PushMatrix();
  #[link_name = "glPopMatrix"]
  fn gl_PopMatrix();
  #[link_name = "glMultMatrixf"]
  fn gl_MultMatrixf(m: *const f32);
  #[link_name = "glEnable"]
  fn gl_Enable(cap: u32);
  #[link_name = "glDisable"]
  fn gl_Disable(cap: u32);
  #[link_name = "glLightfv"]
  fn gl_Lightfv(light: u32, pname: u32, params: *const f32);
  #[link_name = "glLightModelfv"]
  fn gl_LightModelfv(pname: u32, params: *const f32);
  #[link_name = "glLightModeli"]
  fn gl_LightModeli(pname: u32, param: i32);
  #[link_name = "glColorMaterial"]
  fn gl_ColorMaterial(face: u32, mode: u32);
  #[link_name = "glShadeModel"]
  fn gl_ShadeModel(mode: u32);
  #[link_name = "glDepthFunc"]
  fn gl_DepthFunc(func: u32);
  #[link_name = "glBegin"]
  fn gl_Begin(mode: u32);
  #[link_name = "glEnd"]
  fn gl_End();
  #[link_name = "glVertex3f"]
  fn gl_Vertex3f(x: f32, y: f32, z: f32);
  #[link_name = "glColor3f"]
  fn gl_Color3f(r: f32, g: f32, b: f32);
  #[link_name = "glLineWidth"]
  fn gl_LineWidth(width: f32);
  #[link_name = "glEnableClientState"]
  fn gl_EnableClientState(array: u32);
  #[link_name = "glDisableClientState"]
  fn gl_DisableClientState(array: u32);
  #[link_name = "glVertexPointer"]
  fn gl_VertexPointer(
    size: i32,
    type_: u32,
    stride: i32,
    pointer: *const c_void,
  );
  #[link_name = "glNormalPointer"]
  fn gl_NormalPointer(type_: u32, stride: i32, pointer: *const c_void);
  #[link_name = "glDrawArrays"]
  fn gl_DrawArrays(mode: u32, first: i32, count: i32);
  #[link_name = "glClear"]
  fn gl_Clear(mask: u32);
  #[link_name = "glClearColor"]
  fn gl_ClearColor(r: f32, g: f32, b: f32, a: f32);
  #[link_name = "glClearDepth"]
  fn gl_ClearDepth(depth: f64);
  #[link_name = "glClearStencil"]
  fn gl_ClearStencil(s: i32);
  #[link_name = "glViewport"]
  fn gl_Viewport(x: i32, y: i32, width: i32, height: i32);
  #[link_name = "glMaterialfv"]
  fn gl_Materialfv(face: u32, pname: u32, params: *const f32);
  #[link_name = "glMaterialf"]
  fn gl_Materialf(face: u32, pname: u32, param: f32);
  #[link_name = "glUseProgram"]
  fn gl_UseProgram(program: u32);
  #[link_name = "glBindVertexArray"]
  fn gl_BindVertexArray(array: u32);
}

#[cfg(target_os = "windows")]
#[link(name = "opengl32")]
unsafe extern "C" {
  #[link_name = "glMatrixMode"]
  fn gl_MatrixMode(mode: u32);
  #[link_name = "glLoadMatrixf"]
  fn gl_LoadMatrixf(m: *const f32);
  #[link_name = "glLoadIdentity"]
  fn gl_LoadIdentity();
  #[link_name = "glPushMatrix"]
  fn gl_PushMatrix();
  #[link_name = "glPopMatrix"]
  fn gl_PopMatrix();
  #[link_name = "glMultMatrixf"]
  fn gl_MultMatrixf(m: *const f32);
  #[link_name = "glEnable"]
  fn gl_Enable(cap: u32);
  #[link_name = "glDisable"]
  fn gl_Disable(cap: u32);
  #[link_name = "glLightfv"]
  fn gl_Lightfv(light: u32, pname: u32, params: *const f32);
  #[link_name = "glLightModelfv"]
  fn gl_LightModelfv(pname: u32, params: *const f32);
  #[link_name = "glLightModeli"]
  fn gl_LightModeli(pname: u32, param: i32);
  #[link_name = "glColorMaterial"]
  fn gl_ColorMaterial(face: u32, mode: u32);
  #[link_name = "glShadeModel"]
  fn gl_ShadeModel(mode: u32);
  #[link_name = "glDepthFunc"]
  fn gl_DepthFunc(func: u32);
  #[link_name = "glBegin"]
  fn gl_Begin(mode: u32);
  #[link_name = "glEnd"]
  fn gl_End();
  #[link_name = "glVertex3f"]
  fn gl_Vertex3f(x: f32, y: f32, z: f32);
  #[link_name = "glColor3f"]
  fn gl_Color3f(r: f32, g: f32, b: f32);
  #[link_name = "glLineWidth"]
  fn gl_LineWidth(width: f32);
  #[link_name = "glEnableClientState"]
  fn gl_EnableClientState(array: u32);
  #[link_name = "glDisableClientState"]
  fn gl_DisableClientState(array: u32);
  #[link_name = "glVertexPointer"]
  fn gl_VertexPointer(
    size: i32,
    type_: u32,
    stride: i32,
    pointer: *const c_void,
  );
  #[link_name = "glNormalPointer"]
  fn gl_NormalPointer(type_: u32, stride: i32, pointer: *const c_void);
  #[link_name = "glDrawArrays"]
  fn gl_DrawArrays(mode: u32, first: i32, count: i32);
  #[link_name = "glClear"]
  fn gl_Clear(mask: u32);
  #[link_name = "glClearColor"]
  fn gl_ClearColor(r: f32, g: f32, b: f32, a: f32);
  #[link_name = "glClearDepth"]
  fn gl_ClearDepth(depth: f64);
  #[link_name = "glClearStencil"]
  fn gl_ClearStencil(s: i32);
  #[link_name = "glViewport"]
  fn gl_Viewport(x: i32, y: i32, width: i32, height: i32);
  #[link_name = "glMaterialfv"]
  fn gl_Materialfv(face: u32, pname: u32, params: *const f32);
  #[link_name = "glMaterialf"]
  fn gl_Materialf(face: u32, pname: u32, param: f32);
}

// glUseProgram is GL 2.0+ and not exported by opengl32.lib on Windows.
// It must be loaded at runtime via wglGetProcAddress.
#[cfg(target_os = "windows")]
unsafe fn gl_UseProgram(program: u32) {
  use std::sync::OnceLock;
  #[link(name = "opengl32")]
  unsafe extern "C" {
    fn wglGetProcAddress(name: *const std::ffi::c_char) -> *const c_void;
  }
  static FUNC: OnceLock<unsafe extern "C" fn(u32)> = OnceLock::new();
  let f = FUNC.get_or_init(|| {
    let ptr = unsafe { wglGetProcAddress(c"glUseProgram".as_ptr()) };
    assert!(!ptr.is_null(), "failed to load glUseProgram");
    unsafe { std::mem::transmute(ptr) }
  });
  unsafe { f(program) }
}

// glBindVertexArray is GL 3.0+ and not exported by opengl32.lib on Windows.
#[cfg(target_os = "windows")]
unsafe fn gl_BindVertexArray(array: u32) {
  use std::sync::OnceLock;
  #[link(name = "opengl32")]
  unsafe extern "C" {
    fn wglGetProcAddress(name: *const std::ffi::c_char) -> *const c_void;
  }
  static FUNC: OnceLock<unsafe extern "C" fn(u32)> = OnceLock::new();
  let f = FUNC.get_or_init(|| {
    let ptr = unsafe { wglGetProcAddress(c"glBindVertexArray".as_ptr()) };
    assert!(!ptr.is_null(), "failed to load glBindVertexArray");
    unsafe { std::mem::transmute(ptr) }
  });
  unsafe { f(array) }
}

// GL constants
const GL_PROJECTION: u32 = 0x1701;
const GL_MODELVIEW: u32 = 0x1700;
const GL_LIGHTING: u32 = 0x0B50;
const GL_LIGHT0: u32 = 0x4000;
const GL_LIGHT1: u32 = 0x4001;
const GL_LIGHT2: u32 = 0x4002;
const GL_NORMALIZE: u32 = 0x0BA1;
const GL_COLOR_MATERIAL: u32 = 0x0B57;
const GL_POSITION: u32 = 0x1203;
const GL_DIFFUSE: u32 = 0x1201;
const GL_AMBIENT: u32 = 0x1200;
const GL_SPECULAR: u32 = 0x1202;
const GL_SHININESS: u32 = 0x1601;
const GL_AMBIENT_AND_DIFFUSE: u32 = 0x1602;
const GL_FRONT_AND_BACK: u32 = 0x0408;
const GL_LIGHT_MODEL_AMBIENT: u32 = 0x0B53;
const GL_LIGHT_MODEL_TWO_SIDE: u32 = 0x0B52;
const GL_SMOOTH: u32 = 0x1D01;
const GL_EQUAL: u32 = 0x0202;
const GL_LEQUAL: u32 = 0x0203;
const GL_LESS: u32 = 0x0201;
const GL_TRIANGLES: u32 = 0x0004;
const GL_LINES: u32 = 0x0001;
const GL_FLOAT: u32 = 0x1406;
const GL_VERTEX_ARRAY: u32 = 0x8074;
const GL_NORMAL_ARRAY: u32 = 0x8075;
const GL_DEPTH_BUFFER_BIT: u32 = 0x00000100;
const GL_COLOR_BUFFER_BIT: u32 = 0x00004000;
const GL_STENCIL_BUFFER_BIT: u32 = 0x00000400;
const GL_DEPTH_TEST: u32 = 0x0B71;

/// Clear the framebuffer with a background color and reset depth + stencil.
pub fn gl_clear_screen(r: f32, g: f32, b: f32) {
  unsafe {
    gl_ClearColor(r, g, b, 1.0);
    gl_ClearDepth(1.0);
    gl_ClearStencil(0);
    gl_Clear(GL_COLOR_BUFFER_BIT | GL_DEPTH_BUFFER_BIT | GL_STENCIL_BUFFER_BIT);
    gl_Enable(GL_DEPTH_TEST);
    gl_DepthFunc(GL_LESS);
  }
}

/// Set the GL viewport.
pub fn gl_set_viewport(x: i32, y: i32, w: i32, h: i32) {
  unsafe {
    gl_Viewport(x, y, w, h);
  }
}
