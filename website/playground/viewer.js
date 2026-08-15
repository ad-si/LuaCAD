// A small WebGL2 viewer for the meshes the engine returns.
//
// It carries no dependencies on purpose: the whole thing is a flat-shaded
// pass over indexed triangles plus the axes, which is less code than wiring up
// a scene-graph library would be.
//
// Coordinates stay in LuaCAD's CAD frame — X right, Y back, Z up — so the
// numbers in the viewer match the numbers in the script.

const MODEL_VERTEX_SHADER = `#version 300 es
in vec3 position;
uniform mat4 modelViewProjection;
out vec3 viewPosition;
uniform mat4 modelView;

void main() {
  viewPosition = (modelView * vec4(position, 1.0)).xyz;
  gl_Position = modelViewProjection * vec4(position, 1.0);
}`

// The normal comes from the derivatives of the view-space position, which
// makes every triangle flat-shaded without uploading a normal buffer.
const MODEL_FRAGMENT_SHADER = `#version 300 es
precision highp float;
in vec3 viewPosition;
uniform vec3 color;
out vec4 fragColor;

void main() {
  vec3 normal = normalize(cross(dFdx(viewPosition), dFdy(viewPosition)));
  // Two-sided: a hollow model seen from the inside should still be lit.
  float key = abs(dot(normal, normalize(vec3(0.4, 0.3, 1.0))));
  float fill = abs(dot(normal, normalize(vec3(-0.7, -0.4, 0.1)))) * 0.3;
  float light = clamp(0.18 + 0.72 * key + fill, 0.0, 1.0);
  fragColor = vec4(color * light, 1.0);
}`

// Lines carry their own color, so all three axes go out in one draw call.
const LINE_VERTEX_SHADER = `#version 300 es
in vec3 position;
in vec3 color;
uniform mat4 modelViewProjection;
out vec3 lineColor;

void main() {
  lineColor = color;
  gl_Position = modelViewProjection * vec4(position, 1.0);
}`

const LINE_FRAGMENT_SHADER = `#version 300 es
precision highp float;
in vec3 lineColor;
out vec4 fragColor;

void main() {
  fragColor = vec4(lineColor, 1.0);
}`

const DEFAULT_COLOR = [0.66, 0.71, 0.78]

// X, Y, Z in red, green, blue — the same values Studio's `render_axes` uses,
// so a model looks the same in the browser as it does in the app.
const AXIS_COLORS = [
  [1.0, 0.0, 0.0],
  [0.0, 1.0, 0.0],
  [0.3, 0.3, 1.0],
]

export class Viewer {
  constructor(canvas) {
    this.canvas = canvas
    const gl = canvas.getContext("webgl2", { antialias: true, alpha: false })
    if (!gl) {
      throw new Error(
        "This browser has no WebGL2, which the 3D preview needs.",
      )
    }
    this.gl = gl
    this.modelProgram = createProgram(gl, MODEL_VERTEX_SHADER, MODEL_FRAGMENT_SHADER)
    this.lineProgram = createProgram(gl, LINE_VERTEX_SHADER, LINE_FRAGMENT_SHADER)
    this.meshes = []
    this.buildAxes()

    this.azimuth = -35
    this.elevation = 25
    this.distance = 60
    this.target = [0, 0, 0]
    this.dark = matchMedia("(prefers-color-scheme: dark)").matches

    gl.enable(gl.DEPTH_TEST)
    this.installControls()
    this.resize()
    new ResizeObserver(() => {
      this.resize()
      this.draw()
    }).observe(canvas)
    matchMedia("(prefers-color-scheme: dark)").addEventListener("change", (e) => {
      this.dark = e.matches
      this.draw()
    })
  }

  // --- Geometry -------------------------------------------------------------

  /// Replace the scene. Each mesh is `{vertices, indices, color}`, with
  /// vertices as a flat Float32Array of CAD coordinates.
  setMeshes(meshes) {
    const gl = this.gl
    for (const mesh of this.meshes) {
      gl.deleteVertexArray(mesh.vao)
      gl.deleteBuffer(mesh.vertexBuffer)
      gl.deleteBuffer(mesh.indexBuffer)
    }

    const bounds = { min: [Infinity, Infinity, Infinity], max: [-Infinity, -Infinity, -Infinity] }
    this.meshes = meshes.map((mesh) => {
      const vao = gl.createVertexArray()
      gl.bindVertexArray(vao)

      const vertexBuffer = gl.createBuffer()
      gl.bindBuffer(gl.ARRAY_BUFFER, vertexBuffer)
      gl.bufferData(gl.ARRAY_BUFFER, mesh.vertices, gl.STATIC_DRAW)
      const location = gl.getAttribLocation(this.modelProgram, "position")
      gl.enableVertexAttribArray(location)
      gl.vertexAttribPointer(location, 3, gl.FLOAT, false, 0, 0)

      const indexBuffer = gl.createBuffer()
      gl.bindBuffer(gl.ELEMENT_ARRAY_BUFFER, indexBuffer)
      gl.bufferData(gl.ELEMENT_ARRAY_BUFFER, mesh.indices, gl.STATIC_DRAW)
      gl.bindVertexArray(null)

      for (let i = 0; i < mesh.vertices.length; i += 3) {
        for (let axis = 0; axis < 3; axis += 1) {
          const value = mesh.vertices[i + axis]
          bounds.min[axis] = Math.min(bounds.min[axis], value)
          bounds.max[axis] = Math.max(bounds.max[axis], value)
        }
      }

      return {
        vao,
        vertexBuffer,
        indexBuffer,
        count: mesh.indices.length,
        color: mesh.color ?? DEFAULT_COLOR,
      }
    })

    this.bounds = this.meshes.length > 0 ? bounds : null
  }

  /// Frame the current model.
  fit() {
    if (!this.bounds) {
      return
    }
    const { min, max } = this.bounds
    this.target = [0, 1, 2].map((axis) => (min[axis] + max[axis]) / 2)
    const radius = Math.max(
      1e-3,
      Math.hypot(max[0] - min[0], max[1] - min[1], max[2] - min[2]) / 2,
    )
    // 22.5° is half the vertical field of view set up in `draw`.
    this.distance = (radius * 1.4) / Math.tan((22.5 * Math.PI) / 180)
  }

  /// The X, Y and Z axes through the origin: two vertices per half-axis, with
  /// fixed colors and with positions that `updateAxes` writes every frame.
  buildAxes() {
    const gl = this.gl
    const count = 12

    const colors = []
    for (const color of AXIS_COLORS) {
      // The negative half is dimmed, so which way the axis points stays clear.
      const dim = color.map((channel) => channel * 0.4)
      colors.push(...color, ...color, ...dim, ...dim)
    }

    const vao = gl.createVertexArray()
    gl.bindVertexArray(vao)

    const positions = new Float32Array(count * 3)
    const positionBuffer = gl.createBuffer()
    gl.bindBuffer(gl.ARRAY_BUFFER, positionBuffer)
    gl.bufferData(gl.ARRAY_BUFFER, positions, gl.DYNAMIC_DRAW)
    const position = gl.getAttribLocation(this.lineProgram, "position")
    gl.enableVertexAttribArray(position)
    gl.vertexAttribPointer(position, 3, gl.FLOAT, false, 0, 0)

    const colorBuffer = gl.createBuffer()
    gl.bindBuffer(gl.ARRAY_BUFFER, colorBuffer)
    gl.bufferData(gl.ARRAY_BUFFER, new Float32Array(colors), gl.STATIC_DRAW)
    const color = gl.getAttribLocation(this.lineProgram, "color")
    gl.enableVertexAttribArray(color)
    gl.vertexAttribPointer(color, 3, gl.FLOAT, false, 0, 0)
    gl.bindVertexArray(null)

    this.axes = { vao, positionBuffer, colorBuffer, count, positions }
  }

  /// Stretch each half-axis until it leaves the view.
  ///
  /// Studio sends its axis endpoints to infinity outright (`w = 0`), which says
  /// this in one line. In a browser that lands on a line whose far end is
  /// behind the camera — every ray pointing above a camera that looks down
  /// eventually is — and Chrome's software renderer drops such lines instead of
  /// clipping them. So the endpoint is placed on the near side of the crossing
  /// instead, by which point the ray has long left the edge of the view.
  updateAxes(viewProjection) {
    const gl = this.gl
    const positions = this.axes.positions
    // The fourth row of the matrix gives clip-space w — how far in front of the
    // camera a point is. The world origin sits at `originW`, and a step of one
    // along a half-axis moves it by `stepW`, so the ray is in front of the
    // camera while `originW + t * stepW` stays positive.
    const originW = viewProjection[15]
    // Ten times the far plane: where the frustum, not the line, ends it.
    const limit = this.distance * 1000

    let at = 0
    for (let axis = 0; axis < 3; axis += 1) {
      for (const sign of [1, -1]) {
        const stepW = sign * viewProjection[4 * axis + 3]
        let from = 0
        let to = limit
        if (stepW < 0) {
          to = Math.min(to, (-originW / stepW) * 0.999)
        } else if (stepW > 0) {
          from = Math.max(from, (-originW / stepW) * 1.001)
        }
        if (originW <= 0 && stepW <= 0) {
          // The whole ray is behind the camera: collapse it to a point.
          to = from
        }

        positions.fill(0, at, at + 6)
        positions[at + axis] = sign * from
        positions[at + 3 + axis] = sign * to
        at += 6
      }
    }

    gl.bindBuffer(gl.ARRAY_BUFFER, this.axes.positionBuffer)
    gl.bufferSubData(gl.ARRAY_BUFFER, 0, positions)
  }

  // --- Drawing --------------------------------------------------------------

  resize() {
    const ratio = Math.min(devicePixelRatio || 1, 2)
    const width = Math.max(1, Math.round(this.canvas.clientWidth * ratio))
    const height = Math.max(1, Math.round(this.canvas.clientHeight * ratio))
    if (this.canvas.width !== width || this.canvas.height !== height) {
      this.canvas.width = width
      this.canvas.height = height
    }
  }

  draw() {
    const gl = this.gl
    gl.viewport(0, 0, this.canvas.width, this.canvas.height)
    const background = this.dark ? [0.07, 0.07, 0.08, 1] : [0.9, 0.91, 0.93, 1]
    gl.clearColor(...background)
    gl.clear(gl.COLOR_BUFFER_BIT | gl.DEPTH_BUFFER_BIT)

    const aspect = this.canvas.width / this.canvas.height
    const near = Math.max(this.distance / 1000, 1e-3)
    const projection = perspective((45 * Math.PI) / 180, aspect, near, this.distance * 100)
    const view = lookAt(this.eye(), this.target, [0, 0, 1])
    const viewProjection = multiply(projection, view)

    gl.useProgram(this.lineProgram)
    gl.uniformMatrix4fv(
      gl.getUniformLocation(this.lineProgram, "modelViewProjection"),
      false,
      viewProjection,
    )
    this.updateAxes(viewProjection)
    gl.bindVertexArray(this.axes.vao)
    gl.drawArrays(gl.LINES, 0, this.axes.count)

    gl.useProgram(this.modelProgram)
    gl.uniformMatrix4fv(
      gl.getUniformLocation(this.modelProgram, "modelViewProjection"),
      false,
      viewProjection,
    )
    gl.uniformMatrix4fv(
      gl.getUniformLocation(this.modelProgram, "modelView"),
      false,
      view,
    )
    // A model resting on the build plate is coplanar with the X and Y axes;
    // nudging its fragments towards the camera keeps them from bleeding through.
    gl.enable(gl.POLYGON_OFFSET_FILL)
    gl.polygonOffset(-1, -1)
    const colorUniform = gl.getUniformLocation(this.modelProgram, "color")
    for (const mesh of this.meshes) {
      gl.uniform3fv(colorUniform, mesh.color)
      gl.bindVertexArray(mesh.vao)
      gl.drawElements(gl.TRIANGLES, mesh.count, gl.UNSIGNED_INT, 0)
    }
    gl.disable(gl.POLYGON_OFFSET_FILL)
    gl.bindVertexArray(null)
  }

  eye() {
    const azimuth = (this.azimuth * Math.PI) / 180
    const elevation = (this.elevation * Math.PI) / 180
    return [
      this.target[0] + this.distance * Math.cos(elevation) * Math.sin(azimuth),
      this.target[1] - this.distance * Math.cos(elevation) * Math.cos(azimuth),
      this.target[2] + this.distance * Math.sin(elevation),
    ]
  }

  // --- Camera controls ------------------------------------------------------

  installControls() {
    const canvas = this.canvas
    let dragging = null
    let lastX = 0
    let lastY = 0

    canvas.addEventListener("pointerdown", (event) => {
      dragging = event.shiftKey || event.button === 1 ? "pan" : "orbit"
      lastX = event.clientX
      lastY = event.clientY
      canvas.setPointerCapture(event.pointerId)
    })
    canvas.addEventListener("pointerup", (event) => {
      dragging = null
      canvas.releasePointerCapture(event.pointerId)
    })
    canvas.addEventListener("pointermove", (event) => {
      if (!dragging) {
        return
      }
      const dx = event.clientX - lastX
      const dy = event.clientY - lastY
      lastX = event.clientX
      lastY = event.clientY

      if (dragging === "orbit") {
        // Dragging right swings the camera left, so the model turns with the
        // cursor instead of against it.
        this.azimuth -= dx * 0.4
        this.elevation = Math.max(-89.9, Math.min(89.9, this.elevation + dy * 0.4))
      } else {
        // Pan across the plane the camera faces, scaled so the model tracks
        // the cursor at roughly 1:1.
        const scale = (this.distance * 0.002 * 45) / canvas.clientHeight
        const azimuth = (this.azimuth * Math.PI) / 180
        const right = [Math.cos(azimuth), Math.sin(azimuth), 0]
        const up = [
          -Math.sin(azimuth) * Math.sin((this.elevation * Math.PI) / 180),
          Math.cos(azimuth) * Math.sin((this.elevation * Math.PI) / 180),
          Math.cos((this.elevation * Math.PI) / 180),
        ]
        for (let axis = 0; axis < 3; axis += 1) {
          this.target[axis] += (-dx * right[axis] + dy * up[axis]) * scale * 40
        }
      }
      this.draw()
    })
    canvas.addEventListener(
      "wheel",
      (event) => {
        event.preventDefault()
        this.distance *= Math.exp(event.deltaY * 0.001)
        this.draw()
      },
      { passive: false },
    )
    canvas.addEventListener("dblclick", () => {
      this.fit()
      this.draw()
    })
  }
}

// --- Shader and matrix helpers ----------------------------------------------

function createProgram(gl, vertexSource, fragmentSource) {
  const program = gl.createProgram()
  for (const [type, source] of [
    [gl.VERTEX_SHADER, vertexSource],
    [gl.FRAGMENT_SHADER, fragmentSource],
  ]) {
    const shader = gl.createShader(type)
    gl.shaderSource(shader, source)
    gl.compileShader(shader)
    if (!gl.getShaderParameter(shader, gl.COMPILE_STATUS)) {
      throw new Error(`Shader failed to compile: ${gl.getShaderInfoLog(shader)}`)
    }
    gl.attachShader(program, shader)
    gl.deleteShader(shader)
  }
  gl.linkProgram(program)
  if (!gl.getProgramParameter(program, gl.LINK_STATUS)) {
    throw new Error(`Shader program failed to link: ${gl.getProgramInfoLog(program)}`)
  }
  return program
}

function perspective(fovY, aspect, near, far) {
  const f = 1 / Math.tan(fovY / 2)
  return new Float32Array([
    f / aspect, 0, 0, 0,
    0, f, 0, 0,
    0, 0, (far + near) / (near - far), -1,
    0, 0, (2 * far * near) / (near - far), 0,
  ])
}

function lookAt(eye, target, up) {
  const forward = normalize(subtract(target, eye))
  const right = normalize(cross(forward, up))
  const trueUp = cross(right, forward)
  return new Float32Array([
    right[0], trueUp[0], -forward[0], 0,
    right[1], trueUp[1], -forward[1], 0,
    right[2], trueUp[2], -forward[2], 0,
    -dot(right, eye), -dot(trueUp, eye), dot(forward, eye), 1,
  ])
}

function multiply(a, b) {
  const out = new Float32Array(16)
  for (let column = 0; column < 4; column += 1) {
    for (let row = 0; row < 4; row += 1) {
      let sum = 0
      for (let k = 0; k < 4; k += 1) {
        sum += a[k * 4 + row] * b[column * 4 + k]
      }
      out[column * 4 + row] = sum
    }
  }
  return out
}

const subtract = (a, b) => [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
const dot = (a, b) => a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
const cross = (a, b) => [
  a[1] * b[2] - a[2] * b[1],
  a[2] * b[0] - a[0] * b[2],
  a[0] * b[1] - a[1] * b[0],
]

function normalize(v) {
  const length = Math.hypot(...v) || 1
  return [v[0] / length, v[1] / length, v[2] / length]
}
