-- BOSL2 3D shapes preview demo
-- Each shape is placed on a grid so they're all visible at once.

local sp = 80 -- grid spacing

render(
  bosl.tube { h = 30, ["or"] = 20, wall = 4 }
  + bosl.torus { r_maj = 18, r_min = 5 }:translate(sp, 0, 0)
  + bosl.prismoid { size1 = { 40, 40 }, size2 = { 20, 20 }, h = 30 }:translate(2 * sp, 0, 0)
  + bosl.rect_tube { size = { 40, 40 }, wall = 5, h = 30 }:translate(0, sp, 0)
  + bosl.wedge { { 30, 30, 20 } }:translate(sp, sp, 0)
  + bosl.octahedron { size = 35 }:translate(2 * sp, sp, 0)
  + bosl.pie_slice { r = 25, h = 20, ang = 120 }:translate(0, 2 * sp, 0)
  + bosl.regular_prism { n = 6, r = 20, h = 30 }:translate(sp, 2 * sp, 0)
  + bosl.teardrop { r = 15, h = 20 }:translate(2 * sp, 2 * sp, 0)
  + bosl.onion { r = 15 }:translate(0, 3 * sp, 0)
  + bosl.tube { h = 30, or1 = 25, or2 = 15, wall = 4 }:translate(sp, 3 * sp, 0)
  + bosl.pie_slice { r = 25, h = 15, ang = 270 }:translate(2 * sp, 3 * sp, 0)
)
