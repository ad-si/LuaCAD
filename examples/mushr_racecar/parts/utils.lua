-- Shared geometry helpers for the MuSHR racecar.
--
-- Ported from misc/racecar_global_utils.scad.
-- Every value is in millimetres.

local M = {}

-- Facet count for curved surfaces. The original hard-codes 100 everywhere;
-- keeping it in one place lets you dial it down for quick previews.
M.fn = 100

M.wall_thickness = 3.0

M.m3_nut_height = 3.0
M.m3_nut_diameter = 5.7
M.m2_5_nut_height = 2.5
M.m2_5_nut_diameter = 5.2

M.m4_screw_head_height = 2.48
M.m4_screw_head_radius = 8.96 / 2
M.m4_screw_shaft_radius = 2.25
M.m3_screw_head_height = 1.7
M.m3_screw_head_radius = 6.2 / 2
M.m3_screw_shaft_radius = 3.5 / 2
M.m2_5_screw_head_height = 1.5
M.m2_5_screw_head_radius = 5.0 / 2
M.m2_5_screw_shaft_radius = 3.0 / 2

M.m3_insert_depth = 5.0
M.m3_insert_radius = 2.0

M.ball_head_length = 6.0
M.ball_head_width = 6.0
M.ball_head_height = 7.35

-- A cylinder centred on the origin, at the module's facet count.
local function cyl(h, r1, r2)
  return cylinder { h = h, r1 = r1, r2 = r2 or r1, center = true, fn = M.fn }
end

M.cyl = cyl

--------------------------------------------------------------------------
-- Fasteners
--------------------------------------------------------------------------

function M.flathead_screw(height, head_height, head_radius, shaft_radius)
  local shaft_length = height - head_height

  return cyl(head_height, shaft_radius, head_radius)
      :translate(0, 0, height / 2 - head_height / 2)
    + cyl(shaft_length, shaft_radius)
      :translate(0, 0, -height / 2 + shaft_length / 2)
end

function M.m4_flathead_screw(height)
  return M.flathead_screw(
    height,
    M.m4_screw_head_height,
    M.m4_screw_head_radius,
    M.m4_screw_shaft_radius
  )
end

function M.m3_flathead_screw(height)
  return M.flathead_screw(
    height,
    M.m3_screw_head_height,
    M.m3_screw_head_radius,
    M.m3_screw_shaft_radius
  )
end

function M.m2_5_flathead_screw(height)
  return M.flathead_screw(
    height,
    M.m2_5_screw_head_height,
    M.m2_5_screw_head_radius,
    M.m2_5_screw_shaft_radius
  )
end

-- Screw shaft plus the hexagonal pocket its nut drops into.
function M.nut_insert(screw_height, screw_radius, nut_height, nut_diameter)
  return cyl(screw_height, screw_radius)
    + M.hexagon(nut_height, nut_diameter)
      :translate(
        0,
        0,
        -screw_height / 2 + M.wall_thickness + nut_height / 2
      )
end

-- As above, with a slot cut sideways so the nut can be slid in after printing.
function M.nut_insert_with_access(
  access_length,
  screw_height,
  screw_radius,
  nut_height,
  nut_diameter
)
  return M.nut_insert(screw_height, screw_radius, nut_height, nut_diameter)
    + cube { { access_length, nut_diameter, nut_height }, center = true }
      :translate(
        -access_length / 2,
        0,
        -screw_height / 2 + M.wall_thickness + nut_height / 2
      )
    + M.hexagon(nut_height + M.wall_thickness, nut_diameter)
      :translate(
        -access_length,
        0,
        -screw_height / 2 + (nut_height + M.wall_thickness) / 2
      )
end

function M.m3_nut_insert(screw_height)
  return M.nut_insert(
    screw_height,
    M.m3_screw_shaft_radius,
    M.m3_nut_height,
    M.m3_nut_diameter
  )
end

function M.m2_5_nut_insert(screw_height)
  return M.nut_insert(
    screw_height,
    M.m2_5_screw_shaft_radius,
    M.m2_5_nut_height,
    M.m2_5_nut_diameter
  )
end

function M.m2_5_nut_insert_with_access(access_length, screw_height)
  return M.nut_insert_with_access(
    access_length,
    screw_height,
    M.m2_5_screw_shaft_radius,
    M.m2_5_nut_height,
    M.m2_5_nut_diameter
  )
end

--------------------------------------------------------------------------
-- Primitive shapes
--------------------------------------------------------------------------

-- A bar with semicircular ends, centred on the origin.
function M.rounded_square(length, width, height)
  return cube { { length - width, width, height }, center = true }
    + cyl(height, width / 2):translate(-(length - width) / 2, 0, 0)
    + cyl(height, width / 2):translate((length - width) / 2, 0, 0)
end

-- Diameter is measured face-to-face, not vertex-to-vertex.
function M.hexagon(height, diameter)
  local bar = cube { { diameter / 1.732, diameter, height }, center = true }
  return (bar + bar:rotate(0, 0, 120)):hull()
end

-- Right triangular prism: the right angle sits at the origin, the
-- hypotenuse runs from (0,0,h) to (l,0,0), extruded w along +y.
function M.triangle(l, w, h)
  return polyhedron {
    points = {
      { 0, 0, h }, -- 0  front top corner
      { 0, 0, 0 },
      { l, 0, 0 }, -- 1, 2  front bottom corners
      { 0, w, h }, -- 3  back top corner
      { 0, w, 0 },
      { l, w, 0 }, -- 4, 5  back bottom corners
    },
    -- Vertices of every face are ordered clockwise seen from outside.
    faces = {
      { 0, 2, 1 }, -- top
      { 3, 4, 5 }, -- base
      { 0, 1, 4, 3 }, -- height face
      { 1, 2, 5, 4 }, -- width face
      { 0, 3, 5, 2 }, -- hypotenuse
    },
  }
end

-- Two mirrored triangles forming a symmetric peak, centred on the origin.
function M.bh_triangle(base, height, length)
  local half = M.triangle(base / 2, length, height)
  return half:translate(0, -length / 2, -height / 2)
    + half:rotate(0, 0, 180):translate(0, length / 2, -height / 2)
end

function M.rounded_bh_triangle(base, height, length, rounding_radius)
  rounding_radius = rounding_radius or 1.0
  local pin = cyl(length, rounding_radius):rotate(90, 0, 0)

  return (
    M.bh_triangle(base, height, length)
    + pin:translate(0, 0, height / 2)
    + pin:translate(-base / 2, 0, -height / 2)
    + pin:translate(base / 2, 0, -height / 2)
  ):hull()
end

function M.diamond(length, width, height)
  return M.bh_triangle(width, length / 2, height)
      :rotate(90, 0, 90)
      :translate(length / 4, 0, 0)
    + M.bh_triangle(width, length / 2, height)
      :rotate(-90, 0, 90)
      :translate(-length / 4, 0, 0)
end

function M.rounded_diamond(length, width, height, rounding_radius)
  rounding_radius = rounding_radius or 1.0
  local pin = cyl(height, rounding_radius)

  return (
    M.diamond(length, width, height)
    + pin:translate(length / 2, 0, 0)
    + pin:translate(-length / 2, 0, 0)
    + pin:translate(0, width / 2, 0)
    + pin:translate(0, -width / 2, 0)
  ):hull()
end

-- A wedge whose cross-section tapers from base_width to bottom_width
-- vertically and to top_width along its length.
function M.spec_trap(base_width, top_width, bottom_width, length, height)
  return polyhedron {
    points = {
      { 0, -base_width / 2, 0 },
      { 0, base_width / 2, 0 },
      { 0, -bottom_width / 2, -height },
      { 0, bottom_width / 2, -height },
      { length, -top_width / 2, 0 },
      { length, top_width / 2, 0 },
    },
    faces = {
      { 0, 1, 3, 2 },
      { 0, 2, 4 },
      { 1, 5, 3 },
      { 2, 3, 5, 4 },
      { 0, 4, 5, 1 },
    },
  }:translate(-length / 2, 0, height / 2)
end

-- Like triangle(), but with the length and width arguments swapped.
function M.trap_triangle(l, w, h)
  return polyhedron {
    points = {
      { 0, 0, h },
      { 0, 0, 0 },
      { w, 0, 0 },
      { 0, l, h },
      { 0, l, 0 },
      { w, l, 0 },
    },
    faces = {
      { 0, 2, 1 },
      { 3, 4, 5 },
      { 0, 1, 4, 3 },
      { 1, 2, 5, 4 },
      { 0, 3, 5, 2 },
    },
  }
end

-- A slab of length l that widens from w1 at the top face to w2 at the bottom.
function M.trapezoid(w1, w2, l, h)
  local flank = M.trap_triangle(h, (w2 - w1) / 2, l)

  return cube { { l, w1, h }, center = true }
    + flank:rotate(90, 0, 90):translate(-l / 2, w1 / 2, -h / 2)
    + flank:rotate(-90, 0, -90):translate(-l / 2, -w1 / 2, h / 2)
end

function M.rounded_trapezoid(w1, w2, l, h, rounding_radius)
  rounding_radius = rounding_radius or 1.0
  local pin = cyl(h, rounding_radius)

  return (
    M.trapezoid(w1, w2, l, h)
    + pin:translate(l / 2, w1 / 2, 0)
    + pin:translate(l / 2, -w1 / 2, 0)
    + pin:translate(-l / 2, w2 / 2, 0)
    + pin:translate(-l / 2, -w2 / 2, 0)
  ):hull()
end

-- A block with one edge scooped out by an elliptical cylinder.
function M.rounded_edge(length, width, height)
  return cube { { length, width, height }, center = true }
    - cyl(1.0, 1.0)
      :scale(length, width, height)
      :translate(-length / 2, -width / 2, 0)
end

-- The moulded ball joint that the suspension links snap onto.
function M.ball_head()
  local hexagon_height = 1.2
  local hexagon_diameter = 6.0
  local hexagon_z = hexagon_height / 2 - M.ball_head_height / 2

  local cylinder_radius = 3.5 / 2
  local cylinder_height = 2.0
  local cylinder_z = hexagon_z + hexagon_height / 2 + cylinder_height / 2

  local sphere_radius = 5.75 / 2
  local sphere_z = -sphere_radius + M.ball_head_height / 2

  return M.hexagon(hexagon_height, hexagon_diameter):translate(0, 0, hexagon_z)
    + cyl(cylinder_height, cylinder_radius):translate(0, 0, cylinder_z)
    + sphere { r = sphere_radius, fn = M.fn }:translate(0, 0, sphere_z)
end

function M.quarter_sphere(radius)
  return (
    sphere { r = radius, fn = M.fn }
    - cube { { 2 * radius, 2 * radius, radius }, center = true }
      :translate(0, 0, -radius / 2)
    - cube { { 2 * radius, radius, radius }, center = true }
      :translate(0, -radius / 2, radius / 2)
    - cube { { radius, radius, radius }, center = true }
      :translate(-radius / 2, radius / 2, radius / 2)
  ):translate(-radius / 2, -radius / 2, -radius / 2)
end

--------------------------------------------------------------------------
-- Springs
--
-- Adapted from Mendel90, via the original MuSHR source:
-- https://github.com/nophead/Mendel90/blob/master/scad/vitamins/springs.scad
--------------------------------------------------------------------------

-- One quadrilateral slice of the coil's swept cross-section. Where the
-- circular profile pinches to a point the quad degenerates to a triangle,
-- so those two cases are handled separately.
local function coil_segment(i1, i2, r1, r2, hr)
  local alpha1 = i1 * 360 * r2 / hr
  local alpha2 = i2 * 360 * r2 / hr
  local len1 = sin(acos(i1 * 2 - 1)) * r2
  local len2 = sin(acos(i2 * 2 - 1)) * r2

  if len1 < 0.01 then
    return polygon {
      points = {
        { cos(alpha1) * r1, sin(alpha1) * r1 },
        { cos(alpha2) * (r1 - len2), sin(alpha2) * (r1 - len2) },
        { cos(alpha2) * (r1 + len2), sin(alpha2) * (r1 + len2) },
      },
    }
  end

  if len2 < 0.01 then
    return polygon {
      points = {
        { cos(alpha1) * (r1 + len1), sin(alpha1) * (r1 + len1) },
        { cos(alpha1) * (r1 - len1), sin(alpha1) * (r1 - len1) },
        { cos(alpha2) * r1, sin(alpha2) * r1 },
      },
    }
  end

  return polygon {
    points = {
      { cos(alpha1) * (r1 + len1), sin(alpha1) * (r1 + len1) },
      { cos(alpha1) * (r1 - len1), sin(alpha1) * (r1 - len1) },
      { cos(alpha2) * (r1 - len2), sin(alpha2) * (r1 - len2) },
      { cos(alpha2) * (r1 + len2), sin(alpha2) * (r1 + len2) },
    },
  }
end

-- A helical coil: a ring of segments twisted as it is extruded.
function M.coil(r1, r2, h, twists)
  local hr = h / (twists * 2)
  local stepsize = 1 / 16

  local profile = nil
  local i = stepsize
  while i <= 1 + stepsize / 2 do
    local segment = coil_segment(i - stepsize, min(i, 1), r1, r2, hr)
    profile = profile and (profile + segment) or segment
    i = i + stepsize
  end

  -- The original leaves slicing to OpenSCAD's $fn, which for a twisted
  -- extrude means "fragments per full turn". Six turns at ~44 fragments is
  -- ~267 slices, so scale by the total twist rather than passing $fn
  -- straight through -- otherwise the helix is far too coarse.
  local twist = 180 * h / hr
  local fragments_per_turn = (hr / r2) / stepsize

  return profile:linear_extrude(h, {
    twist = twist,
    slices = max(2, floor(abs(twist) / 360 * fragments_per_turn + 0.5)),
  })
end

-- `spec` is { outer_diameter, wire_gauge, free_length, coils }.
function M.comp_spring(spec, length)
  local od, gauge, free_length, coils = spec[1], spec[2], spec[3], spec[4]
  length = (not length or length == 0) and free_length or length

  return M.coil((od - gauge) / 2, gauge / 2, length, coils)
end

return M
