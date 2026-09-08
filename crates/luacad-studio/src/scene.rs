//! The studio's side of the 3D preview.
//!
//! The scene itself is drawn by [`luacad_preview::render`]: image-based CSG
//! through WebCSG, shaded on the wgpu device egui paints with. What lives
//! here is what only the studio needs — the offscreen texture egui shows as
//! an image in the viewport area, and the camera that looks at the model.

use crate::app::AppState;
use crate::camera::*;
use luacad_preview::render;
use std::sync::Arc;

pub use luacad_preview::render::{SCENE_FORMAT, SSAA_FACTOR, SceneView};
pub use luacad_preview::tree::{
  fit_distance_for_extent, solids_extent as compute_scene_extent,
};

/// Renders the 3D scene into a texture that egui paints.
pub struct SceneRenderer {
  device: wgpu::Device,
  egui_renderer: Arc<egui::mutex::RwLock<egui_wgpu::Renderer>>,
  inner: render::SceneRenderer,
  color: wgpu::Texture,
  texture_id: egui::TextureId,
}

impl SceneRenderer {
  /// Create the renderer on egui's device, with a scene texture of the given
  /// size in pixels.
  pub fn new(
    render_state: &egui_wgpu::RenderState,
    width: u32,
    height: u32,
  ) -> Self {
    let device = render_state.device.clone();
    let inner = render::SceneRenderer::new(
      &device,
      &render_state.queue,
      SCENE_FORMAT,
      width,
      height,
    );
    let (width, height) = inner.size();
    let color = render::create_scene_texture(&device, width, height);
    let texture_id = render_state.renderer.write().register_native_texture(
      &device,
      &color.create_view(&Default::default()),
      wgpu::FilterMode::Linear,
    );

    Self {
      device,
      egui_renderer: render_state.renderer.clone(),
      inner,
      color,
      texture_id,
    }
  }

  /// The egui texture the scene is rendered into.
  pub fn texture_id(&self) -> egui::TextureId {
    self.texture_id
  }

  /// Resize the scene texture if the size changed. Returns true if it did;
  /// the texture's content is undefined then.
  pub fn ensure_size(&mut self, width: u32, height: u32) -> bool {
    if !self.inner.ensure_size(width, height) {
      return false;
    }
    let (width, height) = self.inner.size();
    self.color = render::create_scene_texture(&self.device, width, height);
    self
      .egui_renderer
      .write()
      .update_egui_texture_from_wgpu_texture(
        &self.device,
        &self.color.create_view(&Default::default()),
        wgpu::FilterMode::Linear,
        self.texture_id,
      );
    true
  }

  /// Block until the GPU has finished everything submitted so far. Only used
  /// to time renders.
  pub fn wait_idle(&self) {
    self.inner.wait_idle();
  }

  /// Render the scene into the texture.
  pub fn render(&mut self, view: &SceneView) {
    let target = self.color.create_view(&Default::default());
    for error in self.inner.render(&target, view) {
      eprintln!("CSG rendering failed: {error}");
    }
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

  let [tx, ty, tz] = app.camera_target;
  let target = vec3(tx, ty, tz);
  let position = target + vec3(x, y, z);
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

/// Extract projection matrix as column-major f32 array from the camera.
pub fn camera_projection_matrix(camera: &Camera) -> [f32; 16] {
  let m: [[f32; 4]; 4] = camera.projection().into();
  bytemuck::cast(m)
}

/// Extract view matrix as column-major f32 array from the camera.
pub fn camera_view_matrix(camera: &Camera) -> [f32; 16] {
  let m: [[f32; 4]; 4] = camera.view().into();
  bytemuck::cast(m)
}
