-- Basic usage of modules, color, and resolution.
-- Ported from CSG-modules.scad by Marius Kintel.

local debug = true -- Show helper geometry
local fn = 72 -- High segment count for smooth surfaces

-- Core geometric primitives
local body = sphere{r = 10, fn = fn}:color("#26A69A")
local intersector = 
  cube{15, 15, 15, center = true}
  :color("#ffa726")
local hole_object = 
  cylinder{h = 20, r = 5, center = true, fn = fn}
  :color("#3177be")

-- Various modules for visualizing intermediate components
local intersected = body * intersector
local hole_a = hole_object:rotate(0, 90, 0)
local hole_b = hole_object:rotate(90, 0, 0)
local hole_c = hole_object
local holes = hole_a + hole_b + hole_c

-- Main geometry
render(((body * intersector) - holes):translate(0, 0, 25))

-- Helpers
if debug then
  local line =
    cylinder{r = 1, h = 10, center = true, fn = fn}
    :color("black")
  
  -- Intersection breakdown
  local left_group = (intersected
      + body:translate(-15, 0, -35)
      + intersector:translate(15, 0, -35)
      + line:rotate(0, 30, 0):translate(-7.5, 0, -17.5)
      + line:rotate(0, -30, 0):translate(7.5, 0, -17.5)
    ):translate(-30, 0, -40)

  -- Holes breakdown
  local right_group = (holes
      + hole_a:translate(-10, 0, -35)
      + hole_b:translate(10, 0, -35)
      + hole_c:translate(30, 0, -35)
      + line:rotate(0, -20, 0):translate(5, 0, -17.5)
      + line:rotate(0, 30, 0):translate(-5, 0, -17.5)
      + line:rotate(0, -45, 0):translate(15, 0, -17.5)
    ):translate(30, 0, -40)

  local connecting_lines = 
    line:rotate(0, 45, 0):translate(-20, 0, -22.5)
    + line:rotate(0, -45, 0):translate(20, 0, -22.5)

  local helpers = 
    (left_group + right_group + connecting_lines)
    :scale(0.5, 0.5, 0.5)

  render(helpers:translate(0, 0, 25))
end
