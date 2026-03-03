use cgmath::{
  Deg, EuclideanSpace, InnerSpace, Matrix4, MetricSpace, Point3, SquareMatrix,
  Vector3, ortho, perspective,
};

pub type Vec3 = Vector3<f32>;
pub type Mat4 = Matrix4<f32>;

pub fn vec3(x: f32, y: f32, z: f32) -> Vec3 {
  Vector3::new(x, y, z)
}

pub fn degrees(v: f32) -> Deg<f32> {
  Deg(v)
}

/// A pixel coordinate in physical pixels (x from left, y from bottom).
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct PixelPoint {
  pub x: f32,
  pub y: f32,
}

/// Viewport in physical pixels.
#[derive(Debug, Copy, Clone, PartialEq)]
pub struct Viewport {
  pub x: i32,
  pub y: i32,
  pub width: u32,
  pub height: u32,
}

impl Viewport {
  pub fn new_at_origo(width: u32, height: u32) -> Self {
    Self {
      x: 0,
      y: 0,
      width,
      height,
    }
  }

  pub fn aspect(&self) -> f32 {
    self.width as f32 / self.height as f32
  }
}

#[derive(Clone, Debug)]
enum ProjectionType {
  Orthographic { height: f32 },
  Perspective { fov_y: Deg<f32> },
}

#[derive(Clone, Debug)]
pub struct Camera {
  viewport: Viewport,
  projection_type: ProjectionType,
  z_near: f32,
  z_far: f32,
  position: Vec3,
  target: Vec3,
  up: Vec3,
  view: Mat4,
  projection: Mat4,
}

impl Camera {
  pub fn new_perspective(
    viewport: Viewport,
    position: Vec3,
    target: Vec3,
    up: Vec3,
    fov_y: Deg<f32>,
    z_near: f32,
    z_far: f32,
  ) -> Self {
    let mut camera = Self::new(viewport);
    camera.set_view(position, target, up);
    camera.set_perspective_projection(fov_y, z_near, z_far);
    camera
  }

  pub fn new_orthographic(
    viewport: Viewport,
    position: Vec3,
    target: Vec3,
    up: Vec3,
    height: f32,
    z_near: f32,
    z_far: f32,
  ) -> Self {
    let mut camera = Self::new(viewport);
    camera.set_view(position, target, up);
    camera.set_orthographic_projection(height, z_near, z_far);
    camera
  }

  fn new(viewport: Viewport) -> Self {
    Self {
      viewport,
      projection_type: ProjectionType::Orthographic { height: 1.0 },
      z_near: 0.0,
      z_far: 0.0,
      position: vec3(0.0, 0.0, 5.0),
      target: vec3(0.0, 0.0, 0.0),
      up: vec3(0.0, 1.0, 0.0),
      view: Mat4::identity(),
      projection: Mat4::identity(),
    }
  }

  pub fn set_perspective_projection(
    &mut self,
    fov_y: Deg<f32>,
    z_near: f32,
    z_far: f32,
  ) {
    self.z_near = z_near;
    self.z_far = z_far;
    self.projection_type = ProjectionType::Perspective { fov_y };
    self.projection = perspective(fov_y, self.viewport.aspect(), z_near, z_far);
  }

  pub fn set_orthographic_projection(
    &mut self,
    height: f32,
    z_near: f32,
    z_far: f32,
  ) {
    self.projection_type = ProjectionType::Orthographic { height };
    self.z_near = z_near;
    self.z_far = z_far;
    let zoom = self.position.distance(self.target);
    let h = zoom * height;
    let w = h * self.viewport.aspect();
    self.projection =
      ortho(-0.5 * w, 0.5 * w, -0.5 * h, 0.5 * h, z_near, z_far);
  }

  pub fn set_viewport(&mut self, viewport: Viewport) {
    if self.viewport != viewport {
      self.viewport = viewport;
      match self.projection_type {
        ProjectionType::Orthographic { height } => {
          self.set_orthographic_projection(height, self.z_near, self.z_far);
        }
        ProjectionType::Perspective { fov_y } => {
          self.set_perspective_projection(fov_y, self.z_near, self.z_far);
        }
      }
    }
  }

  pub fn set_view(&mut self, position: Vec3, target: Vec3, up: Vec3) {
    self.position = position;
    self.target = target;
    self.up = up.normalize();
    self.view = Mat4::look_at_rh(
      Point3::from_vec(self.position),
      Point3::from_vec(self.target),
      self.up,
    );
    if let ProjectionType::Orthographic { height } = self.projection_type {
      self.set_orthographic_projection(height, self.z_near, self.z_far);
    }
  }

  pub fn projection(&self) -> Mat4 {
    self.projection
  }

  pub fn view(&self) -> Mat4 {
    self.view
  }

  pub fn viewport(&self) -> Viewport {
    self.viewport
  }

  /// Project a world position to pixel coordinates (physical pixels, y from bottom).
  pub fn pixel_at_position(&self, position: Vec3) -> PixelPoint {
    let proj = self.projection * self.view * position.extend(1.0);
    let u = 0.5 * (proj.x / proj.w.abs() + 1.0);
    let v = 0.5 * (proj.y / proj.w.abs() + 1.0);
    PixelPoint {
      x: u * self.viewport.width as f32 + self.viewport.x as f32,
      y: v * self.viewport.height as f32 + self.viewport.y as f32,
    }
  }
}
