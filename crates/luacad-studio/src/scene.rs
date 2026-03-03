use three_d::*;

use crate::app::AppState;
use crate::csg_tree::CsgGroup;
use luacad::geometry::CsgGeometry;
use opencsg_sys::OcsgPrimitive;
use std::ffi::c_void;

/// Data passed to the OpenCSG render callback for each leaf primitive.
struct LeafRenderData {
  vertex_count: usize,
  transform: [f32; 16],
  /// VBO holding vertex positions (3 floats per vertex).
  vbo_vertices: u32,
  /// VBO holding per-vertex normals (3 floats per vertex, one face normal per vertex).
  vbo_normals: u32,
}

/// OpenCSG render callback: draws the leaf's triangulated geometry using VBOs.
unsafe extern "C" fn render_leaf_callback(user_data: *mut c_void) {
  let data = unsafe { &*(user_data as *const LeafRenderData) };

  unsafe {
    gl_PushMatrix();
    gl_MultMatrixf(data.transform.as_ptr());

    // Use VBOs (server-side buffer objects) instead of immediate mode or
    // client-side vertex arrays. Client-side arrays don't work on some
    // GL 4.5 Compatibility contexts (e.g. llvmpipe on aarch64 Linux), and
    // immediate mode inside OpenCSG's FBO causes CSG artifacts on llvmpipe.
    gl_BindBuffer(GL_ARRAY_BUFFER, data.vbo_vertices);
    gl_EnableClientState(GL_VERTEX_ARRAY);
    gl_VertexPointer(3, GL_FLOAT, 0, std::ptr::null());
    gl_DrawArrays(GL_TRIANGLES, 0, data.vertex_count as i32);
    gl_DisableClientState(GL_VERTEX_ARRAY);
    gl_BindBuffer(GL_ARRAY_BUFFER, 0);

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
    let normals = compute_face_normals(&leaf.vertices);

    // Create VBOs for vertex positions and normals (server-side buffer objects).
    // VBOs work correctly on all GL contexts including llvmpipe, unlike
    // client-side vertex arrays which fail on some GL 4.5 Compatibility contexts.
    let mut vbos = [0u32; 2];
    unsafe {
      gl_GenBuffers(2, vbos.as_mut_ptr());

      // Upload vertex positions
      gl_BindBuffer(GL_ARRAY_BUFFER, vbos[0]);
      gl_BufferData(
        GL_ARRAY_BUFFER,
        (leaf.vertices.len() * std::mem::size_of::<[f32; 3]>()) as isize,
        leaf.vertices.as_ptr() as *const c_void,
        GL_STATIC_DRAW,
      );

      // Upload normals
      gl_BindBuffer(GL_ARRAY_BUFFER, vbos[1]);
      gl_BufferData(
        GL_ARRAY_BUFFER,
        (normals.len() * std::mem::size_of::<[f32; 3]>()) as isize,
        normals.as_ptr() as *const c_void,
        GL_STATIC_DRAW,
      );

      gl_BindBuffer(GL_ARRAY_BUFFER, 0);
    }

    render_datas.push(LeafRenderData {
      vertex_count: leaf.vertices.len(),
      transform: cad_to_gl_transform(&leaf.transform),
      vbo_vertices: vbos[0],
      vbo_normals: vbos[1],
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

  // --- OpenCSG depth pass ---
  unsafe {
    opencsg_sys::render(&mut ocsg_prims);
  }

  // --- Shading pass with GL_EQUAL ---
  // OpenCSG's glPopAttrib restores pre-render GL state. Re-render the same
  // geometry with GL_EQUAL to shade only CSG-visible surfaces.
  unsafe {
    gl_UseProgram(0);
    gl_MatrixMode(GL_PROJECTION);
    gl_LoadMatrixf(projection.as_ptr());
    gl_MatrixMode(GL_MODELVIEW);
    gl_LoadMatrixf(view.as_ptr());
    gl_DepthFunc(GL_EQUAL);
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

      // Use VBOs for the shading pass (must match the OpenCSG depth pass
      // to produce identical depth values for GL_EQUAL to work).
      gl_EnableClientState(GL_VERTEX_ARRAY);
      gl_EnableClientState(GL_NORMAL_ARRAY);

      gl_BindBuffer(GL_ARRAY_BUFFER, data.vbo_vertices);
      gl_VertexPointer(3, GL_FLOAT, 0, std::ptr::null());

      gl_BindBuffer(GL_ARRAY_BUFFER, data.vbo_normals);
      gl_NormalPointer(GL_FLOAT, 0, std::ptr::null());

      gl_DrawArrays(GL_TRIANGLES, 0, data.vertex_count as i32);

      gl_DisableClientState(GL_NORMAL_ARRAY);
      gl_DisableClientState(GL_VERTEX_ARRAY);
      gl_BindBuffer(GL_ARRAY_BUFFER, 0);

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

  // Delete VBOs
  for data in &render_datas {
    unsafe {
      let vbos = [data.vbo_vertices, data.vbo_normals];
      gl_DeleteBuffers(2, vbos.as_ptr());
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
    let scad = match geom.scad.as_ref() {
      Some(s) => s,
      None => continue,
    };
    let manifold = luacad::export::materialize_scad_manifold(scad);
    if manifold.num_tri() == 0 {
      continue;
    }
    let (bb_min, bb_max) = manifold.bounding_box();
    // Check all 8 corners, converting CAD (x,y,z) → GL (y,z,x)
    for &cx in &[bb_min[0], bb_max[0]] {
      for &cy in &[bb_min[1], bb_max[1]] {
        for &cz in &[bb_min[2], bb_max[2]] {
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
  #[link_name = "glGenBuffers"]
  fn gl_GenBuffers(n: i32, buffers: *mut u32);
  #[link_name = "glDeleteBuffers"]
  fn gl_DeleteBuffers(n: i32, buffers: *const u32);
  #[link_name = "glBindBuffer"]
  fn gl_BindBuffer(target: u32, buffer: u32);
  #[link_name = "glBufferData"]
  fn gl_BufferData(target: u32, size: isize, data: *const c_void, usage: u32);
  #[link_name = "glVertexPointer"]
  fn gl_VertexPointer(
    size: i32,
    type_: u32,
    stride: i32,
    pointer: *const c_void,
  );
  #[link_name = "glNormalPointer"]
  fn gl_NormalPointer(type_: u32, stride: i32, pointer: *const c_void);
  #[link_name = "glEnableClientState"]
  fn gl_EnableClientState(array: u32);
  #[link_name = "glDisableClientState"]
  fn gl_DisableClientState(array: u32);
  #[link_name = "glDrawArrays"]
  fn gl_DrawArrays(mode: u32, first: i32, count: i32);

  // FBO functions (EXT on macOS GL 2.1 Legacy)
  #[link_name = "glGenFramebuffersEXT"]
  fn gl_GenFramebuffers(n: i32, framebuffers: *mut u32);
  #[link_name = "glDeleteFramebuffersEXT"]
  fn gl_DeleteFramebuffers(n: i32, framebuffers: *const u32);
  #[link_name = "glBindFramebufferEXT"]
  fn gl_BindFramebuffer(target: u32, framebuffer: u32);
  #[link_name = "glGenRenderbuffersEXT"]
  fn gl_GenRenderbuffers(n: i32, renderbuffers: *mut u32);
  #[link_name = "glDeleteRenderbuffersEXT"]
  fn gl_DeleteRenderbuffers(n: i32, renderbuffers: *const u32);
  #[link_name = "glBindRenderbufferEXT"]
  fn gl_BindRenderbuffer(target: u32, renderbuffer: u32);
  #[link_name = "glRenderbufferStorageEXT"]
  fn gl_RenderbufferStorage(target: u32, format: u32, width: i32, height: i32);
  #[link_name = "glFramebufferRenderbufferEXT"]
  fn gl_FramebufferRenderbuffer(
    target: u32,
    attachment: u32,
    renderbuffer_target: u32,
    renderbuffer: u32,
  );
  #[link_name = "glCheckFramebufferStatusEXT"]
  fn gl_CheckFramebufferStatus(target: u32) -> u32;
  #[link_name = "glBlitFramebufferEXT"]
  fn gl_BlitFramebuffer(
    src_x0: i32,
    src_y0: i32,
    src_x1: i32,
    src_y1: i32,
    dst_x0: i32,
    dst_y0: i32,
    dst_x1: i32,
    dst_y1: i32,
    mask: u32,
    filter: u32,
  );
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
  #[link_name = "glGenBuffers"]
  fn gl_GenBuffers(n: i32, buffers: *mut u32);
  #[link_name = "glDeleteBuffers"]
  fn gl_DeleteBuffers(n: i32, buffers: *const u32);
  #[link_name = "glBindBuffer"]
  fn gl_BindBuffer(target: u32, buffer: u32);
  #[link_name = "glBufferData"]
  fn gl_BufferData(target: u32, size: isize, data: *const c_void, usage: u32);
  #[link_name = "glVertexPointer"]
  fn gl_VertexPointer(
    size: i32,
    type_: u32,
    stride: i32,
    pointer: *const c_void,
  );
  #[link_name = "glNormalPointer"]
  fn gl_NormalPointer(type_: u32, stride: i32, pointer: *const c_void);
  #[link_name = "glEnableClientState"]
  fn gl_EnableClientState(array: u32);
  #[link_name = "glDisableClientState"]
  fn gl_DisableClientState(array: u32);
  #[link_name = "glDrawArrays"]
  fn gl_DrawArrays(mode: u32, first: i32, count: i32);

  // FBO functions (core in GL 3.0+, available on Linux)
  #[link_name = "glGenFramebuffers"]
  fn gl_GenFramebuffers(n: i32, framebuffers: *mut u32);
  #[link_name = "glDeleteFramebuffers"]
  fn gl_DeleteFramebuffers(n: i32, framebuffers: *const u32);
  #[link_name = "glBindFramebuffer"]
  fn gl_BindFramebuffer(target: u32, framebuffer: u32);
  #[link_name = "glGenRenderbuffers"]
  fn gl_GenRenderbuffers(n: i32, renderbuffers: *mut u32);
  #[link_name = "glDeleteRenderbuffers"]
  fn gl_DeleteRenderbuffers(n: i32, renderbuffers: *const u32);
  #[link_name = "glBindRenderbuffer"]
  fn gl_BindRenderbuffer(target: u32, renderbuffer: u32);
  #[link_name = "glRenderbufferStorage"]
  fn gl_RenderbufferStorage(target: u32, format: u32, width: i32, height: i32);
  #[link_name = "glFramebufferRenderbuffer"]
  fn gl_FramebufferRenderbuffer(
    target: u32,
    attachment: u32,
    renderbuffer_target: u32,
    renderbuffer: u32,
  );
  #[link_name = "glCheckFramebufferStatus"]
  fn gl_CheckFramebufferStatus(target: u32) -> u32;
  #[link_name = "glBlitFramebuffer"]
  fn gl_BlitFramebuffer(
    src_x0: i32,
    src_y0: i32,
    src_x1: i32,
    src_y1: i32,
    dst_x0: i32,
    dst_y0: i32,
    dst_x1: i32,
    dst_y1: i32,
    mask: u32,
    filter: u32,
  );
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
  #[link_name = "glVertexPointer"]
  fn gl_VertexPointer(
    size: i32,
    type_: u32,
    stride: i32,
    pointer: *const c_void,
  );
  #[link_name = "glNormalPointer"]
  fn gl_NormalPointer(type_: u32, stride: i32, pointer: *const c_void);
  #[link_name = "glEnableClientState"]
  fn gl_EnableClientState(array: u32);
  #[link_name = "glDisableClientState"]
  fn gl_DisableClientState(array: u32);
  #[link_name = "glDrawArrays"]
  fn gl_DrawArrays(mode: u32, first: i32, count: i32);
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

// GL 1.5+ buffer functions are not exported by opengl32.lib on Windows.
#[cfg(target_os = "windows")]
mod win_gl_buffers {
  use std::ffi::c_void;
  use std::sync::OnceLock;

  #[link(name = "opengl32")]
  unsafe extern "C" {
    fn wglGetProcAddress(name: *const std::ffi::c_char) -> *const c_void;
  }

  macro_rules! load_gl_fn {
    ($name:ident, $c_name:expr, $sig:ty) => {
      pub unsafe fn $name() -> $sig {
        static FUNC: OnceLock<$sig> = OnceLock::new();
        *FUNC.get_or_init(|| {
          let ptr = unsafe { wglGetProcAddress($c_name.as_ptr()) };
          assert!(
            !ptr.is_null(),
            concat!("failed to load ", stringify!($name))
          );
          unsafe { std::mem::transmute(ptr) }
        })
      }
    };
  }

  load_gl_fn!(
    gen_buffers,
    c"glGenBuffers",
    unsafe extern "C" fn(i32, *mut u32)
  );
  load_gl_fn!(
    delete_buffers,
    c"glDeleteBuffers",
    unsafe extern "C" fn(i32, *const u32)
  );
  load_gl_fn!(bind_buffer, c"glBindBuffer", unsafe extern "C" fn(u32, u32));
  load_gl_fn!(
    buffer_data,
    c"glBufferData",
    unsafe extern "C" fn(u32, isize, *const c_void, u32)
  );

  // FBO functions (GL 3.0+)
  load_gl_fn!(
    gen_framebuffers,
    c"glGenFramebuffers",
    unsafe extern "C" fn(i32, *mut u32)
  );
  load_gl_fn!(
    delete_framebuffers,
    c"glDeleteFramebuffers",
    unsafe extern "C" fn(i32, *const u32)
  );
  load_gl_fn!(
    bind_framebuffer,
    c"glBindFramebuffer",
    unsafe extern "C" fn(u32, u32)
  );
  load_gl_fn!(
    gen_renderbuffers,
    c"glGenRenderbuffers",
    unsafe extern "C" fn(i32, *mut u32)
  );
  load_gl_fn!(
    delete_renderbuffers,
    c"glDeleteRenderbuffers",
    unsafe extern "C" fn(i32, *const u32)
  );
  load_gl_fn!(
    bind_renderbuffer,
    c"glBindRenderbuffer",
    unsafe extern "C" fn(u32, u32)
  );
  load_gl_fn!(
    renderbuffer_storage,
    c"glRenderbufferStorage",
    unsafe extern "C" fn(u32, u32, i32, i32)
  );
  load_gl_fn!(
    framebuffer_renderbuffer,
    c"glFramebufferRenderbuffer",
    unsafe extern "C" fn(u32, u32, u32, u32)
  );
  load_gl_fn!(
    check_framebuffer_status,
    c"glCheckFramebufferStatus",
    unsafe extern "C" fn(u32) -> u32
  );
  load_gl_fn!(
    blit_framebuffer,
    c"glBlitFramebuffer",
    unsafe extern "C" fn(i32, i32, i32, i32, i32, i32, i32, i32, u32, u32)
  );
}

#[cfg(target_os = "windows")]
unsafe fn gl_GenBuffers(n: i32, buffers: *mut u32) {
  unsafe { (win_gl_buffers::gen_buffers())(n, buffers) }
}
#[cfg(target_os = "windows")]
unsafe fn gl_DeleteBuffers(n: i32, buffers: *const u32) {
  unsafe { (win_gl_buffers::delete_buffers())(n, buffers) }
}
#[cfg(target_os = "windows")]
unsafe fn gl_BindBuffer(target: u32, buffer: u32) {
  unsafe { (win_gl_buffers::bind_buffer())(target, buffer) }
}
#[cfg(target_os = "windows")]
unsafe fn gl_BufferData(
  target: u32,
  size: isize,
  data: *const c_void,
  usage: u32,
) {
  unsafe { (win_gl_buffers::buffer_data())(target, size, data, usage) }
}
#[cfg(target_os = "windows")]
unsafe fn gl_GenFramebuffers(n: i32, framebuffers: *mut u32) {
  unsafe { (win_gl_buffers::gen_framebuffers())(n, framebuffers) }
}
#[cfg(target_os = "windows")]
unsafe fn gl_DeleteFramebuffers(n: i32, framebuffers: *const u32) {
  unsafe { (win_gl_buffers::delete_framebuffers())(n, framebuffers) }
}
#[cfg(target_os = "windows")]
unsafe fn gl_BindFramebuffer(target: u32, framebuffer: u32) {
  unsafe { (win_gl_buffers::bind_framebuffer())(target, framebuffer) }
}
#[cfg(target_os = "windows")]
unsafe fn gl_GenRenderbuffers(n: i32, renderbuffers: *mut u32) {
  unsafe { (win_gl_buffers::gen_renderbuffers())(n, renderbuffers) }
}
#[cfg(target_os = "windows")]
unsafe fn gl_DeleteRenderbuffers(n: i32, renderbuffers: *const u32) {
  unsafe { (win_gl_buffers::delete_renderbuffers())(n, renderbuffers) }
}
#[cfg(target_os = "windows")]
unsafe fn gl_BindRenderbuffer(target: u32, renderbuffer: u32) {
  unsafe { (win_gl_buffers::bind_renderbuffer())(target, renderbuffer) }
}
#[cfg(target_os = "windows")]
unsafe fn gl_RenderbufferStorage(
  target: u32,
  format: u32,
  width: i32,
  height: i32,
) {
  unsafe {
    (win_gl_buffers::renderbuffer_storage())(target, format, width, height)
  }
}
#[cfg(target_os = "windows")]
unsafe fn gl_FramebufferRenderbuffer(
  target: u32,
  attachment: u32,
  renderbuffer_target: u32,
  renderbuffer: u32,
) {
  unsafe {
    (win_gl_buffers::framebuffer_renderbuffer())(
      target,
      attachment,
      renderbuffer_target,
      renderbuffer,
    )
  }
}
#[cfg(target_os = "windows")]
unsafe fn gl_CheckFramebufferStatus(target: u32) -> u32 {
  unsafe { (win_gl_buffers::check_framebuffer_status())(target) }
}
#[cfg(target_os = "windows")]
unsafe fn gl_BlitFramebuffer(
  src_x0: i32,
  src_y0: i32,
  src_x1: i32,
  src_y1: i32,
  dst_x0: i32,
  dst_y0: i32,
  dst_x1: i32,
  dst_y1: i32,
  mask: u32,
  filter: u32,
) {
  unsafe {
    (win_gl_buffers::blit_framebuffer())(
      src_x0, src_y0, src_x1, src_y1, dst_x0, dst_y0, dst_x1, dst_y1, mask,
      filter,
    )
  }
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
const GL_DEPTH_BUFFER_BIT: u32 = 0x00000100;
const GL_COLOR_BUFFER_BIT: u32 = 0x00004000;
const GL_STENCIL_BUFFER_BIT: u32 = 0x00000400;
const GL_DEPTH_TEST: u32 = 0x0B71;
const GL_ARRAY_BUFFER: u32 = 0x8892;
const GL_STATIC_DRAW: u32 = 0x88E4;
const GL_FLOAT: u32 = 0x1406;
const GL_VERTEX_ARRAY: u32 = 0x8074;
const GL_NORMAL_ARRAY: u32 = 0x8075;

// FBO constants
const GL_FRAMEBUFFER: u32 = 0x8D40;
const GL_READ_FRAMEBUFFER: u32 = 0x8CA8;
const GL_DRAW_FRAMEBUFFER: u32 = 0x8CA9;
const GL_RENDERBUFFER: u32 = 0x8D41;
const GL_COLOR_ATTACHMENT0: u32 = 0x8CE0;
const GL_DEPTH_STENCIL_ATTACHMENT: u32 = 0x821A;
const GL_DEPTH24_STENCIL8: u32 = 0x88F0;
const GL_RGBA8: u32 = 0x8058;
const GL_FRAMEBUFFER_COMPLETE: u32 = 0x8CD5;
const GL_NEAREST: u32 = 0x2600;

/// Offscreen framebuffer for rendering the 3D scene.
///
/// OpenCSG's internal FBO/blit logic assumes the GL viewport starts at (0,0).
/// By rendering into this offscreen FBO at (0,0) with the scene dimensions,
/// then blitting to the correct screen position, we avoid that constraint.
pub struct SceneFbo {
  fbo: u32,
  color_rb: u32,
  depth_stencil_rb: u32,
  width: u32,
  height: u32,
}

impl SceneFbo {
  /// Create a new offscreen FBO with the given dimensions.
  pub fn new(width: u32, height: u32) -> Self {
    let w = width.max(1);
    let h = height.max(1);
    let mut fbo = 0u32;
    let mut rbs = [0u32; 2];
    unsafe {
      gl_GenFramebuffers(1, &mut fbo);
      gl_GenRenderbuffers(2, rbs.as_mut_ptr());

      gl_BindFramebuffer(GL_FRAMEBUFFER, fbo);

      // Color attachment
      gl_BindRenderbuffer(GL_RENDERBUFFER, rbs[0]);
      gl_RenderbufferStorage(GL_RENDERBUFFER, GL_RGBA8, w as i32, h as i32);
      gl_FramebufferRenderbuffer(
        GL_FRAMEBUFFER,
        GL_COLOR_ATTACHMENT0,
        GL_RENDERBUFFER,
        rbs[0],
      );

      // Depth+stencil attachment
      gl_BindRenderbuffer(GL_RENDERBUFFER, rbs[1]);
      gl_RenderbufferStorage(
        GL_RENDERBUFFER,
        GL_DEPTH24_STENCIL8,
        w as i32,
        h as i32,
      );
      gl_FramebufferRenderbuffer(
        GL_FRAMEBUFFER,
        GL_DEPTH_STENCIL_ATTACHMENT,
        GL_RENDERBUFFER,
        rbs[1],
      );

      let status = gl_CheckFramebufferStatus(GL_FRAMEBUFFER);
      assert_eq!(
        status, GL_FRAMEBUFFER_COMPLETE,
        "FBO incomplete: {status:#x}"
      );

      gl_BindRenderbuffer(GL_RENDERBUFFER, 0);
      gl_BindFramebuffer(GL_FRAMEBUFFER, 0);
    }

    Self {
      fbo,
      color_rb: rbs[0],
      depth_stencil_rb: rbs[1],
      width: w,
      height: h,
    }
  }

  /// Resize the FBO if dimensions changed. Returns true if resized.
  pub fn ensure_size(&mut self, width: u32, height: u32) -> bool {
    let w = width.max(1);
    let h = height.max(1);
    if w == self.width && h == self.height {
      return false;
    }
    unsafe {
      gl_BindFramebuffer(GL_FRAMEBUFFER, self.fbo);

      gl_BindRenderbuffer(GL_RENDERBUFFER, self.color_rb);
      gl_RenderbufferStorage(GL_RENDERBUFFER, GL_RGBA8, w as i32, h as i32);

      gl_BindRenderbuffer(GL_RENDERBUFFER, self.depth_stencil_rb);
      gl_RenderbufferStorage(
        GL_RENDERBUFFER,
        GL_DEPTH24_STENCIL8,
        w as i32,
        h as i32,
      );

      gl_BindRenderbuffer(GL_RENDERBUFFER, 0);
      gl_BindFramebuffer(GL_FRAMEBUFFER, 0);
    }
    self.width = w;
    self.height = h;
    true
  }

  /// Bind this FBO for rendering. Sets viewport to (0, 0, w, h).
  pub fn bind(&self) {
    unsafe {
      gl_BindFramebuffer(GL_FRAMEBUFFER, self.fbo);
      gl_Viewport(0, 0, self.width as i32, self.height as i32);
    }
  }

  /// Unbind (switch back to default framebuffer).
  pub fn unbind(&self) {
    unsafe {
      gl_BindFramebuffer(GL_FRAMEBUFFER, 0);
    }
  }

  /// Blit the FBO contents to a region of the default framebuffer.
  /// `dst_x`, `dst_y` are in GL coordinates (bottom-left origin, physical pixels).
  pub fn blit_to_screen(&self, dst_x: i32, dst_y: i32, dst_w: u32, dst_h: u32) {
    unsafe {
      gl_BindFramebuffer(GL_READ_FRAMEBUFFER, self.fbo);
      gl_BindFramebuffer(GL_DRAW_FRAMEBUFFER, 0);
      gl_BlitFramebuffer(
        0,
        0,
        self.width as i32,
        self.height as i32,
        dst_x,
        dst_y,
        dst_x + dst_w as i32,
        dst_y + dst_h as i32,
        GL_COLOR_BUFFER_BIT | GL_DEPTH_BUFFER_BIT | GL_STENCIL_BUFFER_BIT,
        GL_NEAREST,
      );
      gl_BindFramebuffer(GL_READ_FRAMEBUFFER, 0);
    }
  }
}

impl Drop for SceneFbo {
  fn drop(&mut self) {
    unsafe {
      gl_DeleteRenderbuffers(
        2,
        [self.color_rb, self.depth_stencil_rb].as_ptr(),
      );
      gl_DeleteFramebuffers(1, &self.fbo);
    }
  }
}

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
