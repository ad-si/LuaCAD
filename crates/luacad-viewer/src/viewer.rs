//! The viewport itself: a wgpu surface on the page's canvas, the scene
//! decoded from the engine's buffer, and the handful of methods the page
//! drives them with.

use crate::camera::Camera;
use luacad_preview::render::{SSAA_FACTOR, SceneRenderer, SceneView};
use luacad_preview::tree::{CsgScene, fit_distance_for_extent};
use luacad_preview::wire;
use wasm_bindgen::prelude::*;
use web_sys::HtmlCanvasElement;

/// Background of the viewport, matching the playground's page colors.
const LIGHT_BACKGROUND: (f32, f32, f32) = (0.9, 0.91, 0.93);
const DARK_BACKGROUND: (f32, f32, f32) = (0.07, 0.07, 0.08);

/// Everything the page needs to draw a model.
#[wasm_bindgen]
pub struct Viewer {
  surface: wgpu::Surface<'static>,
  surface_config: wgpu::SurfaceConfiguration,
  device: wgpu::Device,
  queue: wgpu::Queue,
  renderer: SceneRenderer,
  scene: CsgScene,
  /// Bumped on every new scene so the renderer knows to re-upload it
  revision: u64,
  camera: Camera,
  dark: bool,
}

/// Set up a viewer on `canvas`.
///
/// Fails when the browser has no WebGPU, which is what the page reports in
/// place of the viewport.
#[wasm_bindgen]
pub async fn create_viewer(
  canvas: HtmlCanvasElement,
) -> Result<Viewer, JsValue> {
  console_error_panic_hook::set_once();

  let (width, height) = (canvas.width().max(1), canvas.height().max(1));
  // WebGPU only. WebGL2 is not a fallback the CSG pass can take: it merges
  // the product into the depth buffer by reading depth texels, and a WGSL
  // `textureLoad` from a depth texture has no GLSL equivalent — the pipeline
  // fails to compile on that backend.
  //
  // No `is_browser_webgpu_supported` probe in front of this: it decides by
  // asking for an adapter of its own, and an adapter request that fails for a
  // passing reason — the GPU process still coming up, another tab holding the
  // device — would then be reported as "this browser has no WebGPU" when the
  // browser has it. The calls below need an adapter anyway, so they are the
  // honest test, and whatever they say arrives in the message.
  let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
    backends: wgpu::Backends::BROWSER_WEBGPU,
    ..wgpu::InstanceDescriptor::new_without_display_handle()
  });
  let surface = instance
    .create_surface(wgpu::SurfaceTarget::Canvas(canvas))
    .map_err(no_webgpu)?;
  let adapter = instance
    .request_adapter(&wgpu::RequestAdapterOptions {
      power_preference: wgpu::PowerPreference::HighPerformance,
      compatible_surface: Some(&surface),
      ..Default::default()
    })
    .await
    .map_err(no_webgpu)?;
  let (device, queue) = adapter
    .request_device(&wgpu::DeviceDescriptor {
      label: Some("LuaCAD Viewer"),
      // The floor every WebGPU implementation clears, and all the preview
      // asks for.
      required_limits: wgpu::Limits::downlevel_webgl2_defaults()
        .using_resolution(adapter.limits()),
      ..Default::default()
    })
    .await
    .map_err(|error| js_error("The GPU refused a device", error))?;

  let capabilities = surface.get_capabilities(&adapter);
  let format = surface_format(&capabilities);
  let surface_config = wgpu::SurfaceConfiguration {
    format,
    width,
    height,
    alpha_mode: wgpu::CompositeAlphaMode::Opaque,
    ..surface
      .get_default_config(&adapter, width, height)
      .ok_or_else(|| {
        JsValue::from_str("The GPU cannot draw into this page's canvas")
      })?
  };
  surface.configure(&device, &surface_config);

  let renderer = SceneRenderer::new(&device, &queue, format, width, height);
  Ok(Viewer {
    surface,
    surface_config,
    device,
    queue,
    renderer,
    scene: CsgScene::default(),
    revision: 0,
    camera: Camera::default(),
    dark: false,
  })
}

#[wasm_bindgen]
impl Viewer {
  /// Replace the model with the scene in `bytes`, as the engine encoded it.
  pub fn set_scene(&mut self, bytes: &[u8]) -> Result<(), JsValue> {
    self.scene = wire::decode(bytes)
      .map_err(|error| js_error("Could not read the scene", error))?;
    self.revision += 1;
    Ok(())
  }

  /// Drop the model, leaving an empty viewport with its axes.
  pub fn clear_scene(&mut self) {
    self.scene = CsgScene::default();
    self.revision += 1;
  }

  /// Triangles in the current scene, for the status line.
  pub fn triangle_count(&self) -> usize {
    self.scene.triangle_count()
  }

  /// CSG products in the current scene, one per render call.
  pub fn part_count(&self) -> usize {
    self.scene.groups.len()
  }

  /// Resize the drawing buffer to `width` × `height` device pixels.
  ///
  /// The scene is drawn supersampled and filtered back down by the browser,
  /// which is what smooths the silhouettes and the axis lines, so the buffer
  /// is [`SSAA_FACTOR`] times the size the canvas covers on the page.
  pub fn resize(&mut self, width: u32, height: u32) {
    let width = (width * SSAA_FACTOR).max(1);
    let height = (height * SSAA_FACTOR).max(1);
    if width == self.surface_config.width
      && height == self.surface_config.height
    {
      return;
    }
    self.surface_config.width = width;
    self.surface_config.height = height;
    self.surface.configure(&self.device, &self.surface_config);
    self.renderer.ensure_size(width, height);
  }

  /// Turn the camera around the model. Pixels of pointer movement.
  pub fn orbit(&mut self, dx: f32, dy: f32) {
    self.camera.orbit(dx, dy);
  }

  /// Slide the camera and its target across the plane it faces. Pixels of
  /// pointer movement, against a viewport `height` device pixels tall.
  pub fn pan(&mut self, dx: f32, dy: f32, height: f32) {
    self.camera.pan(dx, dy, height);
  }

  /// Move the camera towards or away from its target, by wheel deltas.
  pub fn zoom(&mut self, delta: f32) {
    self.camera.zoom(delta);
  }

  /// Frame the model.
  pub fn fit(&mut self) {
    if let Some(extent) = self.scene.extent() {
      self.camera.frame(fit_distance_for_extent(extent, false));
    }
  }

  /// Follow the page between its light and dark themes.
  pub fn set_dark(&mut self, dark: bool) {
    self.dark = dark;
  }

  /// Draw the current scene from the current camera.
  pub fn render(&mut self) {
    use wgpu::CurrentSurfaceTexture::{Suboptimal, Success};
    let frame = match self.surface.get_current_texture() {
      Success(frame) | Suboptimal(frame) => frame,
      // The canvas was resized or the context was lost between two frames:
      // reconfigure and let the next call draw.
      _ => {
        self.surface.configure(&self.device, &self.surface_config);
        return;
      }
    };
    let target = frame.texture.create_view(&Default::default());
    let (width, height) =
      (self.surface_config.width, self.surface_config.height);
    let errors = self.renderer.render(
      &target,
      &SceneView {
        groups: &self.scene.groups,
        overlays: &self.scene.overlays,
        // The transparent view mode needs the Manifold booleans of the whole
        // model, which the playground does not compute.
        solids: &[],
        scene_revision: self.revision,
        projection: self.camera.projection(width as f32 / height as f32),
        view: self.camera.view(),
        orthographic: false,
        transparent: false,
        background: if self.dark {
          DARK_BACKGROUND
        } else {
          LIGHT_BACKGROUND
        },
        // Past the far plane at any zoom, so the axes span the whole view
        axis_length: 200.0 * self.camera.distance(),
      },
    );
    for error in errors {
      web_sys::console::error_1(
        &format!("CSG rendering failed: {error}").into(),
      );
    }
    self.queue.present(frame);
  }
}

/// The surface format the shading writes into.
///
/// Not an sRGB one where the browser offers a choice: the shader writes
/// display values, exactly as the studio's offscreen texture takes them, so
/// an sRGB target would encode them a second time and wash the preview out.
fn surface_format(
  capabilities: &wgpu::SurfaceCapabilities,
) -> wgpu::TextureFormat {
  capabilities
    .formats
    .iter()
    .copied()
    .find(|format| !format.is_srgb())
    .unwrap_or(capabilities.formats[0])
}

fn js_error(context: &str, error: impl std::fmt::Display) -> JsValue {
  JsValue::from_str(&format!("{context}: {error}"))
}

/// No WebGPU to draw on: what the page shows in place of the viewport.
///
/// The reason wgpu gave goes to the console rather than into the page —
/// "no WebGPU at all" and "WebGPU, but not for this page right now" are
/// different problems behind the same symptom, and the difference belongs
/// where someone is looking for it.
fn no_webgpu(error: impl std::fmt::Display) -> JsValue {
  web_sys::console::error_1(
    &format!("LuaCAD viewer: no WebGPU adapter: {error}").into(),
  );
  JsValue::from_str(
    "No WebGPU in this browser, which the 3D preview needs — in some \
     browsers it has to be enabled first. Everything else works: scripts \
     still run, and models still export.",
  )
}
