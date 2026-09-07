// Shading of the 3D preview.
//
// Reproduces the look of the fixed-function OpenGL lighting the preview used
// before: three directional lights fixed in eye space, a global ambient
// term, Blinn-Phong highlights with the non-local viewer of GL, two-sided
// lighting, and flat shading from per-face normals.

struct Draw {
  // Clip-space transform. Must be applied exactly like WebCSG does
  // (`u.mvp * vec4(position, 1.0)`), so that the depth values of the shading
  // pass match the CSG pass bit for bit.
  mvp: mat4x4<f32>,
  // Eye-space transform of the same primitive
  model_view: mat4x4<f32>,
  // Diffuse (and ambient) color of the surface, with the opacity in `a`
  color: vec4<f32>,
  // Specular color in `rgb`, shininess in `a`
  specular: vec4<f32>,
  // Emitted color in `rgb`, 1.0 in `a` for an orthographic projection
  emission: vec4<f32>,
  // x: half line width in pixels, y: axis length, zw: viewport size in pixels
  line: vec4<f32>,
};

@group(0) @binding(0)
var<uniform> u: Draw;

const AMBIENT: f32 = 0.35;

// Light directions (towards the light) and diffuse intensities, in eye
// space: the lights stay put relative to the camera when it orbits.
const KEY_DIRECTION: vec3<f32> = vec3<f32>(1.0, 1.0, 0.5);
const KEY_DIFFUSE: f32 = 0.9;
const KEY_SPECULAR: f32 = 0.6;
const FILL_DIRECTION: vec3<f32> = vec3<f32>(-1.0, 0.3, 0.5);
const FILL_DIFFUSE: f32 = 0.55;
const BOTTOM_DIRECTION: vec3<f32> = vec3<f32>(0.0, -1.0, 0.0);
const BOTTOM_DIFFUSE: f32 = 0.4;

struct VertexOutput {
  @invariant @builtin(position) position: vec4<f32>,
  @location(0) eye_position: vec3<f32>,
};

@vertex
fn vs_main(@location(0) position: vec3<f32>) -> VertexOutput {
  var out: VertexOutput;
  out.position = u.mvp * vec4<f32>(position, 1.0);
  let eye = u.model_view * vec4<f32>(position, 1.0);
  out.eye_position = eye.xyz / eye.w;
  return out;
}

fn diffuse(normal: vec3<f32>, direction: vec3<f32>) -> f32 {
  return max(dot(normal, normalize(direction)), 0.0);
}

@fragment
fn fs_lit(in: VertexOutput) -> @location(0) vec4<f32> {
  let p = in.eye_position;
  // Flat shading: the face normal from the screen-space derivatives of the
  // eye-space position. Flipped towards the viewer, which is what GL's
  // two-sided lighting did for the inner surfaces of subtracted primitives.
  var n = normalize(cross(dpdx(p), dpdy(p)));
  let to_viewer = select(-p, vec3<f32>(0.0, 0.0, 1.0), u.emission.a > 0.5);
  if dot(n, to_viewer) < 0.0 {
    n = -n;
  }

  let base = u.color.rgb;
  var color = u.emission.rgb + AMBIENT * base;

  let key = diffuse(n, KEY_DIRECTION);
  color += key * KEY_DIFFUSE * base;
  if key > 0.0 {
    // GL's non-local viewer: the half vector uses the fixed eye direction
    let half = normalize(normalize(KEY_DIRECTION) + vec3<f32>(0.0, 0.0, 1.0));
    let highlight = pow(max(dot(n, half), 0.0), u.specular.a);
    color += highlight * KEY_SPECULAR * u.specular.rgb;
  }
  color += diffuse(n, FILL_DIRECTION) * FILL_DIFFUSE * base;
  color += diffuse(n, BOTTOM_DIRECTION) * BOTTOM_DIFFUSE * base;

  return vec4<f32>(saturate(color), u.color.a);
}

// ---------------------------------------------------------------------------
// Axis lines, drawn as screen-space quads because WebGPU lines are one
// pixel wide

struct LineVertex {
  @location(0) start: vec3<f32>,
  @location(1) end: vec3<f32>,
  @location(2) color: vec3<f32>,
  // x: 0 at the start, 1 at the end of the line; y: -1 or 1, the side
  @location(3) corner: vec2<f32>,
};

struct LineOutput {
  @builtin(position) position: vec4<f32>,
  @location(0) color: vec3<f32>,
};

@vertex
fn vs_line(v: LineVertex) -> LineOutput {
  let length = u.line.y;
  var a = u.mvp * vec4<f32>(v.start * length, 1.0);
  var b = u.mvp * vec4<f32>(v.end * length, 1.0);

  // Keep both ends in front of the camera, so that their projection is
  // meaningful: clip the segment at a tiny positive w.
  let min_w = 1e-4;
  if a.w < min_w {
    a = mix(a, b, (min_w - a.w) / (b.w - a.w));
  }
  if b.w < min_w {
    b = mix(b, a, (min_w - b.w) / (a.w - b.w));
  }

  let viewport = u.line.zw;
  let screen_a = a.xy / a.w * viewport;
  let screen_b = b.xy / b.w * viewport;
  var direction = screen_b - screen_a;
  if dot(direction, direction) < 1e-12 {
    direction = vec2<f32>(1.0, 0.0);
  }
  direction = normalize(direction);
  let normal = vec2<f32>(-direction.y, direction.x);
  // Pixel offset, converted back to normalized device coordinates
  let offset = normal * u.line.x * v.corner.y * 2.0 / viewport;

  let end = select(a, b, v.corner.x > 0.5);
  var out: LineOutput;
  out.position = vec4<f32>(end.xy + offset * end.w, end.z, end.w);
  out.color = v.color;
  return out;
}

@fragment
fn fs_line(in: LineOutput) -> @location(0) vec4<f32> {
  return vec4<f32>(in.color, 1.0);
}
