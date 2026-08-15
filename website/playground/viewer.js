// A small WebGL2 viewer for the meshes the engine returns.
//
// It carries no dependencies on purpose: the whole thing is a flat-shaded
// pass over indexed triangles plus a grid, which is less code than wiring up
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

const LINE_VERTEX_SHADER = `#version 300 es
in vec3 position;
uniform mat4 modelViewProjection;

void main() {
  gl_Position = modelViewProjection * vec4(position, 1.0);
}`

const LINE_FRAGMENT_SHADER = `#version 300 es
precision highp float;
uniform vec4 color;
out vec4 fragColor;

void main() {
  fragColor = color;
}`

const DEFAULT_COLOR = [0.66, 0.71, 0.78]

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
    this.grid = null
    this.gridExtent = 0

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
    this.buildGrid()
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

  /// A grid on the XY plane, sized to the model, plus the X and Y axes.
  buildGrid() {
    const gl = this.gl
    if (this.grid) {
      gl.deleteVertexArray(this.grid.vao)
      gl.deleteBuffer(this.grid.buffer)
      this.grid = null
    }
    if (!this.bounds) {
      return
    }

    const span = Math.max(
      Math.abs(this.bounds.max[0]),
      Math.abs(this.bounds.min[0]),
      Math.abs(this.bounds.max[1]),
      Math.abs(this.bounds.min[1]),
      1,
    )
    // A 1-2-5 step keeps the grid at a handful of divisions per side whether
    // the model is a 2 mm bracket or a 2 m frame.
    const rough = span / 4
    const magnitude = 10 ** Math.floor(Math.log10(rough))
    const normalized = rough / magnitude
    const step = (normalized >= 5 ? 5 : normalized >= 2 ? 2 : 1) * magnitude
    const extent = Math.ceil(span / step) * step
    this.gridExtent = extent

    const lines = []
    for (let at = -extent; at <= extent + step / 2; at += step) {
      lines.push(at, -extent, 0, at, extent, 0)
      lines.push(-extent, at, 0, extent, at, 0)
    }
    const vao = gl.createVertexArray()
    gl.bindVertexArray(vao)
    const buffer = gl.createBuffer()
    gl.bindBuffer(gl.ARRAY_BUFFER, buffer)
    gl.bufferData(gl.ARRAY_BUFFER, new Float32Array(lines), gl.STATIC_DRAW)
    const location = gl.getAttribLocation(this.lineProgram, "position")
    gl.enableVertexAttribArray(location)
    gl.vertexAttribPointer(location, 3, gl.FLOAT, false, 0, 0)
    gl.bindVertexArray(null)

    this.grid = { vao, buffer, count: lines.length / 3 }
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

    if (this.grid) {
      gl.useProgram(this.lineProgram)
      gl.uniformMatrix4fv(
        gl.getUniformLocation(this.lineProgram, "modelViewProjection"),
        false,
        viewProjection,
      )
      gl.uniform4fv(
        gl.getUniformLocation(this.lineProgram, "color"),
        this.dark ? [1, 1, 1, 0.12] : [0, 0, 0, 0.12],
      )
      gl.enable(gl.BLEND)
      gl.blendFunc(gl.SRC_ALPHA, gl.ONE_MINUS_SRC_ALPHA)
      gl.bindVertexArray(this.grid.vao)
      gl.drawArrays(gl.LINES, 0, this.grid.count)
      gl.disable(gl.BLEND)
    }

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
    // A model resting on the build plate is coplanar with the grid; nudging
    // its fragments towards the camera keeps the lines from bleeding through.
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
