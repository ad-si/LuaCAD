-- Text is outlined from a system font into a sketch, so it extrudes and
-- combines like any other 2D shape.

-- text3d() outlines and extrudes in one step.
render(text3d("LuaCAD", { size = 12, depth = 2 }))

-- text() returns the sketch, which leaves the extrusion up to you.
local label = text("v1.0", {
  size = 8,
  halign = "center",
  valign = "top",
})
render(label:linear_extrude(1.5):translate(30, -6, 0))

-- Fonts are chosen by family, optionally with a style. An unknown family
-- falls back to the default sans-serif face rather than failing.
render(
  text3d("Bold", {
    size = 10,
    depth = 2,
    font = "DejaVu Sans:style=Bold",
  }):translate(0, -30, 0)
)

-- Engraving is a difference like any other.
local plate = cube(70, 20, 4)
local engraving = text3d("ENGRAVED", { size = 7, depth = 5 })
  :translate(6, 7, 1)
render((plate - engraving):translate(0, -60, 0))
