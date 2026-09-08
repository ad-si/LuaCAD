//! Drawing the flattened scene: image-based CSG through WebCSG, shaded on
//! wgpu.
//!
//! Each [`CsgGroup`] goes through WebCSG, which fills the depth buffer with
//! the visible surface of the CSG product, and is then shaded with an `Equal`
//! depth test so exactly that surface gets colored. Translucent modifier
//! overlays (`#` and `%`) are blended on top; the transparent view mode draws
//! the materialized solids see-through instead of the CSG groups.
//!
//! The renderer draws into a color target the caller owns — the studio's
//! offscreen texture that egui paints as an image, the playground's canvas
//! surface — and keeps the depth buffer, which is its own business, next to
//! its pipelines.

use crate::tree::{CsgGroup, Operation, OverlayMesh, Shading, SolidMesh};
use glam::Mat4;
use std::sync::Arc;
use webcsg::{BoundingBox, Primitive};
use wgpu::util::DeviceExt;

/// The scene is rendered at this multiple of its on-screen size and filtered
/// back down when it is presented, which smooths the silhouettes and the axis
/// lines. WebCSG works on single-sampled targets, so multisampling is not an
/// option for the CSG pass.
///
/// Costs `SSAA_FACTOR²` in fill rate and texture memory, which is why both
/// front ends render only when something changed.
pub const SSAA_FACTOR: u32 = 2;

/// Opacity of an object in the transparent view mode.
const TRANSPARENT_ALPHA: f32 = 0.4;

/// Color format of the studio's offscreen scene texture. Not sRGB: the
/// shading writes display values, like the fixed-function GL preview did, and
/// egui samples the texture as is.
pub const SCENE_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;

/// Positions only, three floats per vertex, at shader location 0 as WebCSG
/// requires.
const VERTEX_LAYOUT: wgpu::VertexBufferLayout<'static> =
  wgpu::VertexBufferLayout {
    array_stride: 12,
    step_mode: wgpu::VertexStepMode::Vertex,
    attributes: &[wgpu::VertexAttribute {
      offset: 0,
      shader_location: 0,
      format: wgpu::VertexFormat::Float32x3,
    }],
  };

/// Uniforms of one draw call, see `Draw` in `shaders/scene.wgsl`.
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct DrawUniforms {
  mvp: [[f32; 4]; 4],
  model_view: [[f32; 4]; 4],
  color: [f32; 4],
  specular: [f32; 4],
  emission: [f32; 4],
  line: [f32; 4],
}

/// A vertex of the axis line quads, see `LineVertex` in the shader.
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct LineVertex {
  start: [f32; 3],
  end: [f32; 3],
  color: [f32; 3],
  corner: [f32; 2],
}

const LINE_VERTEX_LAYOUT: wgpu::VertexBufferLayout<'static> =
  wgpu::VertexBufferLayout {
    array_stride: std::mem::size_of::<LineVertex>() as u64,
    step_mode: wgpu::VertexStepMode::Vertex,
    attributes: &[
      wgpu::VertexAttribute {
        offset: 0,
        shader_location: 0,
        format: wgpu::VertexFormat::Float32x3,
      },
      wgpu::VertexAttribute {
        offset: 12,
        shader_location: 1,
        format: wgpu::VertexFormat::Float32x3,
      },
      wgpu::VertexAttribute {
        offset: 24,
        shader_location: 2,
        format: wgpu::VertexFormat::Float32x3,
      },
      wgpu::VertexAttribute {
        offset: 36,
        shader_location: 3,
        format: wgpu::VertexFormat::Float32x2,
      },
    ],
  };

/// Maps OpenGL clip space (depth in [-1, 1]) to WebGPU clip space (depth in
/// [0, 1]): z' = 0.5 z + 0.5 w.
const GL_TO_WGPU_DEPTH: Mat4 = Mat4::from_cols_array(&[
  1.0, 0.0, 0.0, 0.0, //
  0.0, 1.0, 0.0, 0.0, //
  0.0, 0.0, 0.5, 0.0, //
  0.0, 0.0, 0.5, 1.0, //
]);

/// Everything one render of the scene depends on.
pub struct SceneView<'a> {
  pub groups: &'a [CsgGroup],
  pub overlays: &'a [OverlayMesh],
  pub solids: &'a [SolidMesh],
  /// Bumped whenever the three slices above change
  pub scene_revision: u64,
  /// Column-major projection with OpenGL's depth range, as the camera makes it
  pub projection: [f32; 16],
  /// Column-major view matrix
  pub view: [f32; 16],
  pub orthographic: bool,
  pub transparent: bool,
  pub background: (f32, f32, f32),
  /// Half length of the axis lines, in world units
  pub axis_length: f32,
}

/// The color and depth targets a render writes to.
struct Targets<'a> {
  color: &'a wgpu::TextureView,
  depth: &'a wgpu::TextureView,
}

// ---------------------------------------------------------------------------
// Geometry on the GPU

/// A triangle mesh in a vertex buffer.
struct GpuMesh {
  buffer: wgpu::Buffer,
  vertex_count: u32,
  /// Object-space bounds, `None` for an empty mesh
  bounds: Option<BoundingBox>,
}

impl GpuMesh {
  fn new(device: &wgpu::Device, vertices: &[[f32; 3]]) -> Self {
    let buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
      label: Some("Scene Mesh"),
      contents: bytemuck::cast_slice(vertices),
      usage: wgpu::BufferUsages::VERTEX,
    });
    Self {
      buffer,
      vertex_count: vertices.len() as u32,
      bounds: bounds_of(vertices),
    }
  }
}

fn bounds_of(vertices: &[[f32; 3]]) -> Option<BoundingBox> {
  let mut min = glam::Vec3::splat(f32::INFINITY);
  let mut max = glam::Vec3::splat(f32::NEG_INFINITY);
  for vertex in vertices {
    let v = glam::Vec3::from(*vertex);
    min = min.min(v);
    max = max.max(v);
  }
  (!vertices.is_empty()).then_some(BoundingBox::new(min, max))
}

struct GpuLeaf {
  mesh: GpuMesh,
  /// Model transform in GL space
  transform: Mat4,
  operation: Operation,
  convexity: u32,
  shading: Shading,
}

struct GpuGroup {
  leaves: Vec<GpuLeaf>,
}

struct GpuOverlay {
  mesh: GpuMesh,
  transform: Mat4,
  color: [f32; 4],
}

struct GpuSolid {
  mesh: GpuMesh,
  shading: Shading,
  /// Center of the bounding box, for the back-to-front sort
  center: [f32; 3],
}

/// The scene's geometry, uploaded once per scene revision.
struct GpuScene {
  revision: u64,
  groups: Vec<GpuGroup>,
  overlays: Vec<GpuOverlay>,
  solids: Vec<GpuSolid>,
}

impl GpuScene {
  fn new(device: &wgpu::Device, view: &SceneView) -> Self {
    let groups = view
      .groups
      .iter()
      .map(|group| GpuGroup {
        leaves: group
          .primitives
          .iter()
          .filter(|leaf| !leaf.vertices.is_empty())
          .map(|leaf| GpuLeaf {
            mesh: GpuMesh::new(device, &leaf.vertices),
            transform: Mat4::from_cols_array(&cad_to_gl_transform(
              &leaf.transform,
            )),
            operation: leaf.operation,
            convexity: leaf.convexity,
            shading: leaf.shading,
          })
          .collect(),
      })
      .collect();
    let overlays = view
      .overlays
      .iter()
      .filter(|overlay| !overlay.vertices.is_empty())
      .map(|overlay| GpuOverlay {
        mesh: GpuMesh::new(device, &overlay.vertices),
        transform: Mat4::from_cols_array(&cad_to_gl_transform(
          &overlay.transform,
        )),
        color: overlay.color,
      })
      .collect();
    let solids = view
      .solids
      .iter()
      .filter(|solid| !solid.vertices.is_empty())
      .map(|solid| GpuSolid {
        mesh: GpuMesh::new(device, &solid.vertices),
        shading: solid.shading,
        center: bounding_center(&solid.vertices),
      })
      .collect();
    Self {
      revision: view.scene_revision,
      groups,
      overlays,
      solids,
    }
  }

  /// Number of draw calls a render of this scene makes
  fn draw_count(&self, transparent: bool) -> u32 {
    let meshes = if transparent {
      2 * self.solids.len()
    } else {
      self.groups.iter().map(|g| g.leaves.len()).sum()
    };
    (meshes + self.overlays.len() + 1) as u32
  }
}

/// A CSG leaf as WebCSG sees it: drawn from the shading pass's vertex buffer,
/// so the geometry is uploaded once.
struct LeafPrimitive {
  buffer: wgpu::Buffer,
  vertex_count: u32,
  operation: webcsg::Operation,
  convexity: u32,
  transform: Mat4,
  bounding_box: Option<BoundingBox>,
}

impl Primitive for LeafPrimitive {
  fn operation(&self) -> webcsg::Operation {
    self.operation
  }

  fn set_operation(&mut self, operation: webcsg::Operation) {
    self.operation = operation;
  }

  fn convexity(&self) -> u32 {
    self.convexity
  }

  fn set_convexity(&mut self, convexity: u32) {
    self.convexity = convexity;
  }

  fn bounding_box(&self) -> Option<BoundingBox> {
    self.bounding_box
  }

  fn set_bounding_box(&mut self, bbox: BoundingBox) {
    self.bounding_box = Some(bbox);
  }

  fn vertex_data(&self) -> &[u8] {
    &[]
  }

  fn vertex_buffer(&self) -> Option<&wgpu::Buffer> {
    Some(&self.buffer)
  }

  fn index_data(&self) -> Option<(&[u8], wgpu::IndexFormat)> {
    None
  }

  fn vertex_layout(&self) -> wgpu::VertexBufferLayout<'static> {
    VERTEX_LAYOUT
  }

  fn vertex_count(&self) -> u32 {
    self.vertex_count
  }

  fn index_range(&self) -> Option<std::ops::Range<u32>> {
    None
  }

  fn transform(&self) -> Option<Mat4> {
    Some(self.transform)
  }
}

/// The operation as WebCSG spells it.
fn webcsg_operation(operation: Operation) -> webcsg::Operation {
  match operation {
    Operation::Intersection => webcsg::Operation::Intersection,
    Operation::Subtraction => webcsg::Operation::Subtraction,
  }
}

// ---------------------------------------------------------------------------
// Draw list

/// How a mesh is drawn in the shading pass
#[derive(Clone, Copy)]
enum Style {
  /// Front faces of an intersected primitive where the depth equals the
  /// CSG result
  CsgFront,
  /// Back faces of a subtracted primitive (the walls of the cavity it
  /// carves) where the depth equals the CSG result
  CsgBack,
  /// Ordinary depth-tested mesh, both sides
  Plain,
  /// Blended, far side of a see-through solid
  TransparentBack,
  /// Blended, near side of a see-through solid
  TransparentFront,
  /// Blended over the opaque result, depth-tested but not depth-written
  Overlay,
}

/// One mesh draw of the shading pass
struct Draw<'s> {
  mesh: &'s GpuMesh,
  style: Style,
  /// Index of the draw's uniforms in the uniform buffer
  uniform: u32,
}

/// The passes of one render, in order
enum Step<'s> {
  /// Let WebCSG merge a CSG product into the depth buffer
  Csg(Vec<Arc<dyn Primitive>>),
  /// Shade meshes
  Draws(Vec<Draw<'s>>),
}

/// Collects the uniforms of the draws of one render, in draw order.
struct Uniforms {
  draws: Vec<DrawUniforms>,
  view_projection: Mat4,
  view: Mat4,
  /// 1.0 for an orthographic projection
  orthographic: f32,
  line: [f32; 4],
}

impl Uniforms {
  /// Add a draw's uniforms and return their index
  fn push(&mut self, transform: Mat4, shading: Shading, alpha: f32) -> u32 {
    let [r, g, b] = shading.color;
    let [er, eg, eb] = shading.emission;
    self.draws.push(DrawUniforms {
      // Exactly the product WebCSG computes for its depth pass, so that
      // the `Equal` depth test of the shading hits the CSG surface
      mvp: (self.view_projection * transform).to_cols_array_2d(),
      model_view: (self.view * transform).to_cols_array_2d(),
      color: [r, g, b, alpha],
      specular: shading.specular,
      emission: [er, eg, eb, self.orthographic],
      line: self.line,
    });
    self.draws.len() as u32 - 1
  }
}

/// Build the draw list of a render and its uniforms.
fn build_steps<'s>(
  scene: &'s GpuScene,
  transparent: bool,
  view: &[f32; 16],
  uniforms: &mut Uniforms,
) -> Vec<Step<'s>> {
  let view_projection = uniforms.view_projection;
  let mut steps = Vec::new();

  if transparent {
    let mut draws = Vec::new();
    for index in back_to_front(&scene.solids, view) {
      let solid = &scene.solids[index];
      let uniform =
        uniforms.push(Mat4::IDENTITY, solid.shading, TRANSPARENT_ALPHA);
      // Far side first, so that it is composited under the near side
      draws.push(Draw {
        mesh: &solid.mesh,
        style: Style::TransparentBack,
        uniform,
      });
      draws.push(Draw {
        mesh: &solid.mesh,
        style: Style::TransparentFront,
        uniform,
      });
    }
    steps.push(Step::Draws(draws));
  } else {
    for group in &scene.groups {
      if group.leaves.is_empty() {
        continue;
      }
      // A group with a single intersected primitive is just a plain mesh:
      // the CSG pass would contribute nothing, and its convexity-bounded
      // depth peeling drops surfaces of concave meshes (e.g. a
      // materialized Minkowski minus a cube). Such groups are drawn with
      // ordinary depth testing instead.
      let plain = group.leaves.len() == 1
        && group.leaves[0].operation == Operation::Intersection;
      if !plain {
        let primitives = group
          .leaves
          .iter()
          .map(|leaf| {
            Arc::new(LeafPrimitive {
              buffer: leaf.mesh.buffer.clone(),
              vertex_count: leaf.mesh.vertex_count,
              operation: webcsg_operation(leaf.operation),
              convexity: leaf.convexity,
              transform: leaf.transform,
              bounding_box: leaf
                .mesh
                .bounds
                .map(|b| b.transformed(&(view_projection * leaf.transform))),
            }) as Arc<dyn Primitive>
          })
          .collect();
        steps.push(Step::Csg(primitives));
      }
      let draws = group
        .leaves
        .iter()
        .map(|leaf| Draw {
          mesh: &leaf.mesh,
          style: match (plain, leaf.operation) {
            (true, _) => Style::Plain,
            (false, Operation::Intersection) => Style::CsgFront,
            (false, Operation::Subtraction) => Style::CsgBack,
          },
          uniform: uniforms.push(leaf.transform, leaf.shading, 1.0),
        })
        .collect();
      steps.push(Step::Draws(draws));
    }
  }

  // Translucent modifier meshes over the opaque result
  let overlays = scene
    .overlays
    .iter()
    .map(|overlay| Draw {
      mesh: &overlay.mesh,
      style: Style::Overlay,
      uniform: uniforms.push(
        overlay.transform,
        Shading::plain([overlay.color[0], overlay.color[1], overlay.color[2]]),
        overlay.color[3],
      ),
    })
    .collect();
  steps.push(Step::Draws(overlays));
  steps
}

// ---------------------------------------------------------------------------
// Renderer

/// The shading pipelines, one per [`Style`] plus the axes.
struct Pipelines {
  csg_front: wgpu::RenderPipeline,
  csg_back: wgpu::RenderPipeline,
  plain: wgpu::RenderPipeline,
  transparent_back: wgpu::RenderPipeline,
  transparent_front: wgpu::RenderPipeline,
  overlay: wgpu::RenderPipeline,
  axes: wgpu::RenderPipeline,
}

impl Pipelines {
  fn get(&self, style: Style) -> &wgpu::RenderPipeline {
    match style {
      Style::CsgFront => &self.csg_front,
      Style::CsgBack => &self.csg_back,
      Style::Plain => &self.plain,
      Style::TransparentBack => &self.transparent_back,
      Style::TransparentFront => &self.transparent_front,
      Style::Overlay => &self.overlay,
    }
  }
}

/// Renders the 3D scene into a color target the caller owns.
pub struct SceneRenderer {
  device: wgpu::Device,
  queue: wgpu::Queue,
  csg: webcsg::Context,
  pipelines: Pipelines,
  uniform_layout: wgpu::BindGroupLayout,
  uniform_buffer: wgpu::Buffer,
  uniform_bind_group: wgpu::BindGroup,
  /// Draws the uniform buffer has room for
  uniform_capacity: u32,
  /// Bytes between two draws' uniforms
  uniform_stride: u64,
  axes: wgpu::Buffer,
  depth: wgpu::Texture,
  width: u32,
  height: u32,
  scene: Option<GpuScene>,
}

impl SceneRenderer {
  /// Create the renderer on an existing device, writing into color targets of
  /// `color_format` and `width` × `height` pixels.
  pub fn new(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    color_format: wgpu::TextureFormat,
    width: u32,
    height: u32,
  ) -> Self {
    let device = device.clone();
    let queue = queue.clone();
    let csg = webcsg::Context::new(&device);
    let depth_format = webcsg::recommended_depth_format(&device);

    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
      label: Some("Scene Shader"),
      source: wgpu::ShaderSource::Wgsl(
        include_str!("shaders/scene.wgsl").into(),
      ),
    });
    let uniform_stride = (std::mem::size_of::<DrawUniforms>() as u64)
      .max(device.limits().min_uniform_buffer_offset_alignment as u64);
    let uniform_layout =
      device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("Scene Draw Uniforms Layout"),
        entries: &[wgpu::BindGroupLayoutEntry {
          binding: 0,
          visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
          ty: wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Uniform,
            has_dynamic_offset: true,
            min_binding_size: wgpu::BufferSize::new(std::mem::size_of::<
              DrawUniforms,
            >() as u64),
          },
          count: None,
        }],
      });
    let pipeline_layout =
      device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("Scene Pipeline Layout"),
        bind_group_layouts: &[Some(&uniform_layout)],
        immediate_size: 0,
      });

    let blend = wgpu::BlendState {
      color: wgpu::BlendComponent {
        src_factor: wgpu::BlendFactor::SrcAlpha,
        dst_factor: wgpu::BlendFactor::OneMinusSrcAlpha,
        operation: wgpu::BlendOperation::Add,
      },
      // The target stays opaque: egui would otherwise blend the scene's
      // see-through pixels with whatever it painted underneath
      alpha: wgpu::BlendComponent {
        src_factor: wgpu::BlendFactor::Zero,
        dst_factor: wgpu::BlendFactor::One,
        operation: wgpu::BlendOperation::Add,
      },
    };
    let make_pipeline = |label: &str,
                         entries: (&str, &str),
                         layout: wgpu::VertexBufferLayout<'static>,
                         cull: Option<wgpu::Face>,
                         depth_compare: wgpu::CompareFunction,
                         depth_write: bool,
                         blended: bool| {
      device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some(label),
        layout: Some(&pipeline_layout),
        vertex: wgpu::VertexState {
          module: &shader,
          entry_point: Some(entries.0),
          compilation_options: Default::default(),
          buffers: &[Some(layout)],
        },
        fragment: Some(wgpu::FragmentState {
          module: &shader,
          entry_point: Some(entries.1),
          compilation_options: Default::default(),
          targets: &[Some(wgpu::ColorTargetState {
            format: color_format,
            blend: blended.then_some(blend),
            write_mask: wgpu::ColorWrites::ALL,
          })],
        }),
        primitive: wgpu::PrimitiveState {
          topology: wgpu::PrimitiveTopology::TriangleList,
          front_face: wgpu::FrontFace::Ccw,
          cull_mode: cull,
          ..Default::default()
        },
        depth_stencil: Some(wgpu::DepthStencilState {
          format: depth_format,
          depth_write_enabled: Some(depth_write),
          depth_compare: Some(depth_compare),
          stencil: Default::default(),
          bias: Default::default(),
        }),
        multisample: Default::default(),
        multiview_mask: None,
        cache: None,
      })
    };
    use wgpu::CompareFunction::{Equal, Less, LessEqual};
    use wgpu::Face::{Back, Front};
    let lit = ("vs_main", "fs_lit");
    let pipelines = Pipelines {
      csg_front: make_pipeline(
        "Scene CSG Front",
        lit,
        VERTEX_LAYOUT,
        Some(Back),
        Equal,
        false,
        false,
      ),
      csg_back: make_pipeline(
        "Scene CSG Back",
        lit,
        VERTEX_LAYOUT,
        Some(Front),
        Equal,
        false,
        false,
      ),
      plain: make_pipeline(
        "Scene Plain",
        lit,
        VERTEX_LAYOUT,
        None,
        Less,
        true,
        false,
      ),
      transparent_back: make_pipeline(
        "Scene Transparent Back",
        lit,
        VERTEX_LAYOUT,
        Some(Front),
        Less,
        false,
        true,
      ),
      transparent_front: make_pipeline(
        "Scene Transparent Front",
        lit,
        VERTEX_LAYOUT,
        Some(Back),
        Less,
        false,
        true,
      ),
      // Only the surface facing the viewer: without depth writes, back
      // faces would blend through the front ones and show the far side's
      // tessellation as darker bands
      overlay: make_pipeline(
        "Scene Overlay",
        lit,
        VERTEX_LAYOUT,
        Some(Back),
        LessEqual,
        false,
        true,
      ),
      axes: make_pipeline(
        "Scene Axes",
        ("vs_line", "fs_line"),
        LINE_VERTEX_LAYOUT,
        None,
        Less,
        false,
        false,
      ),
    };

    let axes = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
      label: Some("Scene Axes"),
      contents: bytemuck::cast_slice(&axis_vertices()),
      usage: wgpu::BufferUsages::VERTEX,
    });

    let depth = create_depth_texture(&device, width, height);

    let uniform_capacity = 64;
    let (uniform_buffer, uniform_bind_group) = create_uniform_buffer(
      &device,
      &uniform_layout,
      uniform_stride,
      uniform_capacity,
    );

    Self {
      device,
      queue,
      csg,
      pipelines,
      uniform_layout,
      uniform_buffer,
      uniform_bind_group,
      uniform_capacity,
      uniform_stride,
      axes,
      width: depth.width(),
      height: depth.height(),
      depth,
      scene: None,
    }
  }

  /// Resize the depth buffer if the size changed. Returns true if it did; the
  /// caller's color target has to follow.
  pub fn ensure_size(&mut self, width: u32, height: u32) -> bool {
    let (width, height) = (width.max(1), height.max(1));
    if width == self.width && height == self.height {
      return false;
    }
    self.depth = create_depth_texture(&self.device, width, height);
    self.width = width;
    self.height = height;
    true
  }

  /// Size of the render target, in pixels.
  pub fn size(&self) -> (u32, u32) {
    (self.width, self.height)
  }

  /// Block until the GPU has finished everything submitted so far. Only used
  /// to time renders, and not available on the web, where the browser owns
  /// the event loop.
  pub fn wait_idle(&self) {
    let _ = self.device.poll(wgpu::PollType::wait_indefinitely());
  }

  /// Make room for `draws` sets of uniforms.
  fn ensure_uniform_capacity(&mut self, draws: u32) {
    if draws <= self.uniform_capacity {
      return;
    }
    let capacity = draws.next_power_of_two();
    let (buffer, bind_group) = create_uniform_buffer(
      &self.device,
      &self.uniform_layout,
      self.uniform_stride,
      capacity,
    );
    self.uniform_buffer = buffer;
    self.uniform_bind_group = bind_group;
    self.uniform_capacity = capacity;
  }

  /// Render the scene into `target`, which must have the color format and the
  /// size the renderer was set up with.
  ///
  /// Returns the CSG products WebCSG refused to draw — each one is a hole in
  /// the preview, and where that is worth reporting differs between the front
  /// ends, so the renderer only hands them back.
  pub fn render(
    &mut self,
    target: &wgpu::TextureView,
    view: &SceneView,
  ) -> Vec<String> {
    if self
      .scene
      .as_ref()
      .is_none_or(|scene| scene.revision != view.scene_revision)
    {
      self.scene = Some(GpuScene::new(&self.device, view));
    }
    // The transparent mode falls back to the CSG groups when the scene
    // materialized to nothing, rather than showing nothing
    let transparent = view.transparent
      && self.scene.as_ref().is_some_and(|s| !s.solids.is_empty());
    let draw_count = self
      .scene
      .as_ref()
      .map_or(1, |scene| scene.draw_count(transparent));
    self.ensure_uniform_capacity(draw_count);

    let projection = GL_TO_WGPU_DEPTH * Mat4::from_cols_array(&view.projection);
    let view_matrix = Mat4::from_cols_array(&view.view);
    let mut uniforms = Uniforms {
      draws: Vec::with_capacity(draw_count as usize),
      view_projection: projection * view_matrix,
      view: view_matrix,
      orthographic: if view.orthographic { 1.0 } else { 0.0 },
      line: [
        // Two render pixels wide (one physical pixel after supersampling):
        // half the width, like the GL preview's `glLineWidth(2.0)`
        1.0,
        view.axis_length,
        self.width as f32,
        self.height as f32,
      ],
    };

    let scene = self.scene.as_ref().expect("uploaded above");
    let steps = build_steps(scene, transparent, &view.view, &mut uniforms);
    let axes_uniform =
      uniforms.push(Mat4::IDENTITY, Shading::plain([0.0; 3]), 1.0);

    // --- Upload the uniforms ---
    let stride = self.uniform_stride as usize;
    let mut bytes = vec![0u8; stride * uniforms.draws.len()];
    for (i, draw) in uniforms.draws.iter().enumerate() {
      bytes[i * stride..][..std::mem::size_of::<DrawUniforms>()]
        .copy_from_slice(bytemuck::bytes_of(draw));
    }
    self.queue.write_buffer(&self.uniform_buffer, 0, &bytes);

    let depth_view = self.depth.create_view(&Default::default());
    let targets = Targets {
      color: target,
      depth: &depth_view,
    };
    let (bg_r, bg_g, bg_b) = view.background;
    let background = wgpu::Color {
      r: bg_r as f64,
      g: bg_g as f64,
      b: bg_b as f64,
      a: 1.0,
    };

    // Every render pass boundary costs a load and a store of the (large,
    // supersampled) color and depth attachments, so as many draws as
    // possible go into one pass: the clear happens in the first one, the
    // axes in the last, and only a CSG product, which WebCSG renders with
    // passes of its own, forces a break.
    let record_pass = |encoder: &mut wgpu::CommandEncoder,
                       draws: &[&Draw],
                       clear: bool,
                       axes: bool| {
      let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some("Scene Shading"),
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
          view: targets.color,
          depth_slice: None,
          resolve_target: None,
          ops: wgpu::Operations {
            load: if clear {
              wgpu::LoadOp::Clear(background)
            } else {
              wgpu::LoadOp::Load
            },
            store: wgpu::StoreOp::Store,
          },
        })],
        depth_stencil_attachment: Some(
          wgpu::RenderPassDepthStencilAttachment {
            view: targets.depth,
            depth_ops: Some(wgpu::Operations {
              load: if clear {
                wgpu::LoadOp::Clear(1.0)
              } else {
                wgpu::LoadOp::Load
              },
              store: wgpu::StoreOp::Store,
            }),
            stencil_ops: None,
          },
        ),
        timestamp_writes: None,
        occlusion_query_set: None,
        multiview_mask: None,
      });
      for draw in draws {
        pass.set_pipeline(self.pipelines.get(draw.style));
        pass.set_bind_group(
          0,
          &self.uniform_bind_group,
          &[(draw.uniform as u64 * self.uniform_stride) as u32],
        );
        pass.set_vertex_buffer(0, draw.mesh.buffer.slice(..));
        pass.draw(0..draw.mesh.vertex_count, 0..1);
      }
      if axes {
        pass.set_pipeline(&self.pipelines.axes);
        pass.set_bind_group(
          0,
          &self.uniform_bind_group,
          &[(axes_uniform as u64 * self.uniform_stride) as u32],
        );
        pass.set_vertex_buffer(0, self.axes.slice(..));
        pass.draw(0..AXIS_VERTEX_COUNT, 0..1);
      }
    };

    let csg_options = webcsg::RenderOptions {
      view_projection: uniforms.view_projection,
      ..Default::default()
    };
    let mut encoder = self.device.create_command_encoder(&Default::default());
    let mut pending: Vec<&Draw> = Vec::new();
    let mut cleared = false;
    let mut errors = Vec::new();
    for step in &steps {
      match step {
        Step::Csg(primitives) => {
          record_pass(&mut encoder, &pending, !cleared, false);
          pending.clear();
          cleared = true;
          // WebCSG submits its own command buffers, so everything recorded
          // so far has to go first
          self.queue.submit(std::iter::once(encoder.finish()));
          encoder = self.device.create_command_encoder(&Default::default());
          if let Err(error) = self.csg.render(
            &self.device,
            &self.queue,
            primitives,
            &self.depth,
            &csg_options,
          ) {
            errors.push(error.to_string());
          }
        }
        Step::Draws(draws) => pending.extend(draws),
      }
    }
    record_pass(&mut encoder, &pending, !cleared, true);
    self.queue.submit(std::iter::once(encoder.finish()));
    errors
  }
}

/// The color texture a caller that has no surface of its own — the studio,
/// which hands it to egui — renders into.
pub fn create_scene_texture(
  device: &wgpu::Device,
  width: u32,
  height: u32,
) -> wgpu::Texture {
  device.create_texture(&wgpu::TextureDescriptor {
    label: Some("Scene Color"),
    size: wgpu::Extent3d {
      width: width.max(1),
      height: height.max(1),
      depth_or_array_layers: 1,
    },
    mip_level_count: 1,
    sample_count: 1,
    dimension: wgpu::TextureDimension::D2,
    format: SCENE_FORMAT,
    usage: wgpu::TextureUsages::RENDER_ATTACHMENT
      | wgpu::TextureUsages::TEXTURE_BINDING,
    view_formats: &[],
  })
}

fn create_depth_texture(
  device: &wgpu::Device,
  width: u32,
  height: u32,
) -> wgpu::Texture {
  device.create_texture(&wgpu::TextureDescriptor {
    label: Some("Scene Depth"),
    size: wgpu::Extent3d {
      width: width.max(1),
      height: height.max(1),
      depth_or_array_layers: 1,
    },
    mip_level_count: 1,
    sample_count: 1,
    dimension: wgpu::TextureDimension::D2,
    format: webcsg::recommended_depth_format(device),
    usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
    view_formats: &[],
  })
}

fn create_uniform_buffer(
  device: &wgpu::Device,
  layout: &wgpu::BindGroupLayout,
  stride: u64,
  capacity: u32,
) -> (wgpu::Buffer, wgpu::BindGroup) {
  let buffer = device.create_buffer(&wgpu::BufferDescriptor {
    label: Some("Scene Draw Uniforms"),
    size: stride * capacity as u64,
    usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
    mapped_at_creation: false,
  });
  let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
    label: Some("Scene Draw Uniforms"),
    layout,
    entries: &[wgpu::BindGroupEntry {
      binding: 0,
      resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
        buffer: &buffer,
        offset: 0,
        size: wgpu::BufferSize::new(std::mem::size_of::<DrawUniforms>() as u64),
      }),
    }],
  });
  (buffer, bind_group)
}

/// Vertices of the axis line quads through the origin.
/// CAD convention: Red=X, Green=Y, Blue=Z. Mapping: CAD (x,y,z) → GL (y,z,x).
/// Negative directions are dimmed. The lines are scaled to the axis length
/// in the shader.
fn axis_vertices() -> Vec<LineVertex> {
  // (GL direction, color) per CAD axis
  let axes = [
    ([0.0_f32, 0.0, 1.0], [1.0_f32, 0.0, 0.0]), // CAD X (red) → GL Z
    ([1.0, 0.0, 0.0], [0.0, 1.0, 0.0]),         // CAD Y (green) → GL X
    ([0.0, 1.0, 0.0], [0.3, 0.3, 1.0]),         // CAD Z (blue) → GL Y
  ];
  let corners = [
    [0.0, -1.0],
    [1.0, -1.0],
    [1.0, 1.0],
    [0.0, -1.0],
    [1.0, 1.0],
    [0.0, 1.0],
  ];
  let mut vertices = Vec::new();
  for ([x, y, z], [r, g, b]) in axes {
    for (sign, color) in [(1.0, [r, g, b]), (-1.0, [0.4 * r, 0.4 * g, 0.4 * b])]
    {
      for corner in corners {
        vertices.push(LineVertex {
          start: [0.0; 3],
          end: [sign * x, sign * y, sign * z],
          color,
          corner,
        });
      }
    }
  }
  vertices
}

/// Three axes, two directions each, six vertices per line quad
const AXIS_VERTEX_COUNT: u32 = 3 * 2 * 6;

/// Order the solids from the farthest to the nearest, which is the order the
/// blended transparent pass has to paint them in.
fn back_to_front(solids: &[GpuSolid], view: &[f32; 16]) -> Vec<usize> {
  let centers: Vec<[f32; 3]> = solids.iter().map(|s| s.center).collect();
  back_to_front_centers(&centers, view)
}

/// [`back_to_front`] on the solids' bounding centers.
fn back_to_front_centers(centers: &[[f32; 3]], view: &[f32; 16]) -> Vec<usize> {
  let mut order: Vec<(f32, usize)> = centers
    .iter()
    .enumerate()
    .map(|(index, center)| (transform_point(view, *center)[2], index))
    .collect();
  // The camera looks down -z in view space, so the most negative depth is
  // the farthest solid and has to be painted first.
  order.sort_by(|(a, _), (b, _)| a.total_cmp(b));
  order.into_iter().map(|(_, index)| index).collect()
}

/// Center of the bounding box of a mesh, used to sort the transparent solids
/// back to front. The origin for an empty mesh.
fn bounding_center(vertices: &[[f32; 3]]) -> [f32; 3] {
  let mut min = [f32::INFINITY; 3];
  let mut max = [f32::NEG_INFINITY; 3];
  for vertex in vertices {
    for axis in 0..3 {
      min[axis] = min[axis].min(vertex[axis]);
      max[axis] = max[axis].max(vertex[axis]);
    }
  }
  if min[0] > max[0] {
    return [0.0; 3];
  }
  [
    (min[0] + max[0]) * 0.5,
    (min[1] + max[1]) * 0.5,
    (min[2] + max[2]) * 0.5,
  ]
}

/// Apply a column-major affine 4x4 matrix to a point.
fn transform_point(m: &[f32; 16], p: [f32; 3]) -> [f32; 3] {
  [
    m[0] * p[0] + m[4] * p[1] + m[8] * p[2] + m[12],
    m[1] * p[0] + m[5] * p[1] + m[9] * p[2] + m[13],
    m[2] * p[0] + m[6] * p[1] + m[10] * p[2] + m[14],
  ]
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

#[cfg(test)]
mod tests {
  use super::*;

  const IDENTITY: [f32; 16] = [
    1.0, 0.0, 0.0, 0.0, //
    0.0, 1.0, 0.0, 0.0, //
    0.0, 0.0, 1.0, 0.0, //
    0.0, 0.0, 0.0, 1.0, //
  ];

  /// A camera at the origin looking down -z, with the world pushed 10 units
  /// away from it.
  fn view_matrix() -> [f32; 16] {
    let mut view = IDENTITY;
    view[14] = -10.0;
    view
  }

  /// One triangle in the z plane.
  fn solid_at_z(z: f32) -> SolidMesh {
    SolidMesh {
      vertices: vec![[0.0, 0.0, z], [1.0, 0.0, z], [0.0, 1.0, z]],
      shading: Shading::plain([1.0; 3]),
    }
  }

  /// Blending has no depth test to fall back on, so the far solid has to be
  /// painted before the near one no matter which order the scene lists them.
  #[test]
  fn transparent_solids_are_painted_from_the_back() {
    let centers = [
      bounding_center(&solid_at_z(1.0).vertices),
      bounding_center(&solid_at_z(-1.0).vertices),
    ];
    let order = back_to_front_centers(&centers, &view_matrix());
    assert_eq!(order, vec![1, 0], "the near solid was painted first");
  }

  /// The bounding center is what the transparent sort keys on.
  #[test]
  fn the_bounding_center_is_the_middle_of_the_extent() {
    assert_eq!(
      bounding_center(&[[0.0, 0.0, 2.0], [4.0, 2.0, 2.0]]),
      [2.0, 1.0, 2.0]
    );
    assert_eq!(
      bounding_center(&[]),
      [0.0; 3],
      "an empty mesh sits at the origin"
    );
  }

  /// WebCSG works in WebGPU clip space, where the near plane is at depth 0
  /// rather than at GL's -1.
  #[test]
  fn gl_depth_is_mapped_to_the_webgpu_range() {
    let near = GL_TO_WGPU_DEPTH * glam::Vec4::new(0.0, 0.0, -1.0, 1.0);
    let far = GL_TO_WGPU_DEPTH * glam::Vec4::new(0.0, 0.0, 1.0, 1.0);
    assert_eq!(near.z, 0.0);
    assert_eq!(far.z, 1.0);
  }

  /// The CAD → GL permutation has to move the translation along with the
  /// axes.
  #[test]
  fn cad_transform_permutes_translation() {
    let mut cad = IDENTITY;
    cad[12] = 1.0; // x
    cad[13] = 2.0; // y
    cad[14] = 3.0; // z
    let gl = cad_to_gl_transform(&cad);
    assert_eq!(&gl[12..15], &[2.0, 3.0, 1.0]);
  }
}
