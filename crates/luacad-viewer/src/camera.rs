//! The orbit camera of the playground's viewport.
//!
//! Same controls and same matrices as the studio's camera — 0.5° of rotation
//! per pixel dragged, a pan that keeps the model under the cursor, an
//! exponential zoom — so a model handles the same way in the browser as it
//! does in the app. Everything is in the preview's GL space (Y up), which is
//! the space the flattened scene's vertices already live in.

/// Where the camera starts: the studio's home view.
const DEFAULT_AZIMUTH: f32 = -30.0;
const DEFAULT_ELEVATION: f32 = 30.0;
/// Only until the first model arrives and [`super::Viewer::fit`] frames it.
const DEFAULT_DISTANCE: f32 = 60.0;

/// Vertical field of view of the perspective projection, in degrees.
const FOV_Y: f32 = 45.0;

pub struct Camera {
  azimuth: f32,
  elevation: f32,
  distance: f32,
  target: [f32; 3],
}

impl Default for Camera {
  fn default() -> Self {
    Self {
      azimuth: DEFAULT_AZIMUTH,
      elevation: DEFAULT_ELEVATION,
      distance: DEFAULT_DISTANCE,
      target: [0.0; 3],
    }
  }
}

impl Camera {
  pub fn distance(&self) -> f32 {
    self.distance
  }

  /// Look at the origin from `distance` away, keeping the current angles.
  pub fn frame(&mut self, distance: f32) {
    self.target = [0.0; 3];
    self.distance = distance.clamp(0.001, 10_000.0);
  }

  pub fn orbit(&mut self, dx: f32, dy: f32) {
    // Dragging right swings the camera left, so the model turns with the
    // cursor rather than against it.
    self.azimuth -= dx * 0.5;
    self.elevation = (self.elevation + dy * 0.5).clamp(-85.0, 85.0);
  }

  /// Slide the camera and its target across the plane the camera faces, so
  /// that the model tracks the cursor.
  pub fn pan(&mut self, dx: f32, dy: f32, viewport_height: f32) {
    let visible_height = 2.0 * self.distance * (FOV_Y / 2.0).to_radians().tan();
    let world_per_pixel = visible_height / viewport_height.max(1.0);
    let to_camera = self.to_camera();
    let right = normalize(cross(negate(to_camera), [0.0, 1.0, 0.0]));
    let up = cross(right, negate(to_camera));
    for axis in 0..3 {
      self.target[axis] += right[axis] * (-dx * world_per_pixel)
        + up[axis] * (dy * world_per_pixel);
    }
  }

  /// Move the camera towards or away from its target. `delta` is a wheel
  /// event's `deltaY` in pixels: scrolling down pushes the camera back.
  pub fn zoom(&mut self, delta: f32) {
    self.distance =
      (self.distance * (delta * 0.001).exp()).clamp(0.001, 10_000.0);
  }

  /// Column-major view matrix.
  pub fn view(&self) -> [f32; 16] {
    let to_camera = self.to_camera();
    let eye = [
      self.target[0] + to_camera[0] * self.distance,
      self.target[1] + to_camera[1] * self.distance,
      self.target[2] + to_camera[2] * self.distance,
    ];
    look_at(eye, self.target, [0.0, 1.0, 0.0])
  }

  /// Column-major perspective projection with OpenGL's depth range, which is
  /// what the renderer expects and maps to WebGPU's itself.
  pub fn projection(&self, aspect: f32) -> [f32; 16] {
    let near = 0.1 * self.distance;
    let far = 100.0 * self.distance;
    let f = 1.0 / (FOV_Y.to_radians() / 2.0).tan();
    #[rustfmt::skip]
    let projection = [
      f / aspect.max(1e-6), 0.0, 0.0,                         0.0,
      0.0,                  f,   0.0,                         0.0,
      0.0,                  0.0, (far + near) / (near - far), -1.0,
      0.0,                  0.0, (2.0 * far * near) / (near - far), 0.0,
    ];
    projection
  }

  /// Unit vector from the target towards the camera.
  fn to_camera(&self) -> [f32; 3] {
    let az = self.azimuth.to_radians();
    let el = self.elevation.to_radians();
    [el.cos() * az.sin(), el.sin(), el.cos() * az.cos()]
  }
}

fn look_at(eye: [f32; 3], target: [f32; 3], up: [f32; 3]) -> [f32; 16] {
  let forward = normalize(subtract(target, eye));
  let right = normalize(cross(forward, up));
  let true_up = cross(right, forward);
  #[rustfmt::skip]
  let view = [
    right[0],         true_up[0],         -forward[0],       0.0,
    right[1],         true_up[1],         -forward[1],       0.0,
    right[2],         true_up[2],         -forward[2],       0.0,
    -dot(right, eye), -dot(true_up, eye), dot(forward, eye), 1.0,
  ];
  view
}

fn subtract(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
  [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn negate(v: [f32; 3]) -> [f32; 3] {
  [-v[0], -v[1], -v[2]]
}

fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
  a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
  [
    a[1] * b[2] - a[2] * b[1],
    a[2] * b[0] - a[0] * b[2],
    a[0] * b[1] - a[1] * b[0],
  ]
}

fn normalize(v: [f32; 3]) -> [f32; 3] {
  let length = dot(v, v).sqrt();
  if length < 1e-12 {
    return [0.0, 0.0, 1.0];
  }
  [v[0] / length, v[1] / length, v[2] / length]
}

#[cfg(test)]
mod tests {
  use super::*;

  /// The camera looks at its target: the target has to land on the view
  /// space's negative z axis, `distance` away.
  #[test]
  fn the_view_matrix_puts_the_target_in_front_of_the_camera() {
    let camera = Camera::default();
    let view = camera.view();
    let t = camera.target;
    let z = view[2] * t[0] + view[6] * t[1] + view[10] * t[2] + view[14];
    let x = view[0] * t[0] + view[4] * t[1] + view[8] * t[2] + view[12];
    assert!((z + camera.distance).abs() < 1e-3, "target at z = {z}");
    assert!(x.abs() < 1e-3, "target off the view axis at x = {x}");
  }

  /// Scrolling down (a positive wheel delta in the page's coordinates) has to
  /// move the camera away, like the studio's wheel handler.
  #[test]
  fn zooming_out_increases_the_distance() {
    let mut camera = Camera::default();
    let before = camera.distance();
    camera.zoom(100.0);
    assert!(camera.distance() > before);
    camera.zoom(-100.0);
    assert!((camera.distance() - before).abs() < 1e-3);
  }
}
