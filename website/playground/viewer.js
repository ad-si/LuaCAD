// The 3D preview, driven by the WebAssembly viewer module.
//
// The drawing itself is LuaCAD's own preview — the CSG products the engine
// flattens a script into, rendered by WebCSG on wgpu, exactly as the desktop
// studio renders them. What is left here is what belongs to the page: the
// canvas, the pointer, the wheel, and the theme.
//
// The camera lives in the module (it owns the matrices), so a drag is a call
// into wasm followed by a redraw.

import init, { create_viewer } from "./viewer/luacad_viewer.js"

/// Attach a viewer to `canvas`. Rejects when there is no WebGPU to draw on,
/// which is what the page reports in place of the viewport.
export async function createViewer(canvas) {
  // WebGPU is exposed to secure contexts only, and a page served from an
  // address that is neither loopback nor HTTPS is not one — `http://[::]:8000`,
  // which is what `python3 -m http.server` puts in its banner, is the way to
  // land here by accident. Without this the browser looks like it has no
  // WebGPU at all, which sends the search in the wrong direction entirely.
  if (!window.isSecureContext) {
    throw new Error(
      `This page is served from ${location.origin}, which browsers do not ` +
        "treat as a secure context, so WebGPU — which the 3D preview needs — " +
        "is not available. Open it over localhost, 127.0.0.1 or HTTPS.",
    )
  }
  await init()
  const viewer = await create_viewer(canvas)
  return new Viewer(canvas, viewer)
}

class Viewer {
  constructor(canvas, viewer) {
    this.canvas = canvas
    this.viewer = viewer
    this.frame = null

    const dark = matchMedia("(prefers-color-scheme: dark)")
    this.viewer.set_dark(dark.matches)
    dark.addEventListener("change", (event) => {
      this.viewer.set_dark(event.matches)
      this.draw()
    })

    this.installControls()
    this.resize()
    new ResizeObserver(() => {
      this.resize()
      this.draw()
    }).observe(canvas)
    this.draw()
  }

  // --- Scene ----------------------------------------------------------------

  /// Show the scene the engine encoded, as a `Uint8Array`.
  setScene(bytes) {
    this.viewer.set_scene(bytes)
  }

  clear() {
    this.viewer.clear_scene()
  }

  /// Triangles in the current scene, for the status line.
  triangleCount() {
    return this.viewer.triangle_count()
  }

  /// CSG products in the current scene: one WebCSG render call each.
  partCount() {
    return this.viewer.part_count()
  }

  /// Frame the model.
  fit() {
    this.viewer.fit()
  }

  // --- Drawing --------------------------------------------------------------

  /// Redraw before the next repaint. Several calls in one frame — a drag that
  /// fires twice, a resize next to a new scene — draw once.
  draw() {
    if (this.frame !== null) {
      return
    }
    this.frame = requestAnimationFrame(() => {
      this.frame = null
      this.viewer.render()
    })
  }

  resize() {
    // The module supersamples on top of this, so two is as far as the device
    // pixel ratio is followed: past that the fill rate buys nothing visible.
    const ratio = Math.min(devicePixelRatio || 1, 2)
    const width = Math.max(1, Math.round(this.canvas.clientWidth * ratio))
    const height = Math.max(1, Math.round(this.canvas.clientHeight * ratio))
    this.viewer.resize(width, height)
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
        this.viewer.orbit(dx, dy)
      } else {
        this.viewer.pan(dx, dy, canvas.clientHeight)
      }
      this.draw()
    })
    canvas.addEventListener(
      "wheel",
      (event) => {
        event.preventDefault()
        this.viewer.zoom(event.deltaY)
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
