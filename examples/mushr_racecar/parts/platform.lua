-- The main chassis plate: a 374 mm laser-cut deck with a raised lip, the
-- four suspension mounts, the moulded nose cone, and the bolt pattern
-- every other subassembly screws into.
--
-- Ported from chassis/racecar_chassis_platform.scad.

local u = require("parts.utils")

local M = {}

M.length = 374.0
M.width = 127.0
M.height = 2.54

M.back_length = 86.0
M.back_width = 53.18
M.back_x = -M.length / 2 + M.back_length / 2

local middle_length = 329.0 - M.back_length
local middle_x = M.back_x + M.back_length / 2 + middle_length / 2

M.front_length = M.length - (middle_length + M.back_length)
M.front_width = 52.54
M.front_x = middle_x + middle_length / 2 + M.front_length / 2

local wall_height = 15.33 - M.height

M.suspension_connector_radius = 5.0
local suspension_connector_height = 47.0
M.back_suspension_connector_x = M.back_x
  - M.back_length / 2
  + suspension_connector_height / 2
  + 15.0
M.back_suspension_connector_left_y = M.back_width / 2 - 2.0
M.back_suspension_connector_right_y = -M.back_width / 2 + 2.0
M.back_suspension_connector_z = M.height / 2
  + M.suspension_connector_radius
  - 1.5

M.front_suspension_connector_x = M.front_x
  + M.front_length / 2
  - suspension_connector_height / 2
  - 13.25

--------------------------------------------------------------------------
-- Nose cone
--
-- Three stacked polyhedra -- base, ramp and grille -- topped by a ridge,
-- with vents cut into the sides and front.
--------------------------------------------------------------------------

local base_back_bottom_x = 0.0
local base_front_bottom_x = 10.0
local base_front_top_x = 17.0
local base_left_y = 56.0 / 2
local base_right_y = -56.0 / 2
local base_top_z = 6.25

local ramp_back_bottom_x = base_front_top_x - 3.0
local ramp_front_bottom_x = base_front_top_x
local ramp_back_top_x = ramp_back_bottom_x + 10.0
local ramp_front_top_x = ramp_back_top_x + 3.0
local ramp_left_top_y = base_left_y + 1.0
local ramp_right_top_y = base_right_y - 1.0
local ramp_top_z = base_top_z + 6.67

local grill_back_top_x = ramp_back_top_x + 9.5
local grill_front_top_x = grill_back_top_x + 3.0
local grill_left_top_y = ramp_left_top_y + 1.5
local grill_right_top_y = ramp_right_top_y - 1.5
local grill_top_z = ramp_top_z + 22.0

local tri_base = 4.5
local tri_height = 4
local tri_length = 109.5
local tri_x = grill_back_top_x + tri_height / 2
local tri_z = grill_top_z

-- A box with independent front/back edges top and bottom, so it can lean
-- and flare at the same time. Vertex order matches the original exactly.
local function slab(bx0, fx0, ly0, ry0, z0, bx1, fx1, ly1, ry1, z1)
  return polyhedron {
    points = {
      { bx0, ly0, z0 }, -- 0  bottom back left
      { fx0, ly0, z0 }, -- 1  bottom front left
      { bx0, ry0, z0 }, -- 2  bottom back right
      { fx0, ry0, z0 }, -- 3  bottom front right
      { bx1, ly1, z1 }, -- 4  top back left
      { fx1, ly1, z1 }, -- 5  top front left
      { bx1, ry1, z1 }, -- 6  top back right
      { fx1, ry1, z1 }, -- 7  top front right
    },
    faces = {
      { 1, 0, 2, 3 }, -- bottom
      { 2, 0, 4, 6 }, -- back
      { 4, 5, 7, 6 }, -- top
      { 1, 3, 7, 5 }, -- front
      { 3, 2, 6, 7 }, -- right
      { 0, 1, 5, 4 }, -- left
    },
  }
end

local function front_bumper()
  local top_outer_length = 14.25
  local top_outer_z = tri_z + top_outer_length / 2

  local top_inner_length = top_outer_length - 2.75
  local top_inner_z = top_outer_z - top_outer_length / 2 + top_inner_length / 2

  local dot_x_scale = 3.5
  local dot_y_scale = 5.5
  local dot_x = tri_x - tri_height + 2.0
  local dot_y = 22.0 / 2
  local dot_z = tri_z + tri_base / 2 + dot_y_scale

  local vent_height = grill_front_top_x - base_back_bottom_x
  local vent_x = 0.5 * (grill_front_top_x + base_back_bottom_x)

  local diamond_length = 14.5
  local diamond_y = 37.0 / 2
  local diamond_z = 0.5 * (grill_top_z + ramp_top_z) - diamond_length / 3

  local slot_outer_length = 15.0
  local slot_outer_z = ramp_top_z + slot_outer_length / 2 + 2.0
  local slot_inner_length = 4.0
  local slot_inner_z = slot_outer_z
    + slot_outer_length / 2
    - slot_inner_length / 2
    + 0.5

  -- The two headlamp blisters: a half-sphere stretched into an ellipsoid.
  local blister = (
    sphere { r = 1.0, fn = u.fn }
    - cube { { 1.0, 2.0, 2.0 }, center = true }:translate(-0.5, 0, 0)
  ):scale(dot_x_scale, dot_y_scale, dot_y_scale)

  local body = slab(
    base_back_bottom_x,
    base_front_bottom_x,
    base_left_y,
    base_right_y,
    0.0,
    base_back_bottom_x,
    base_front_top_x,
    base_left_y,
    base_right_y,
    base_top_z
  )
    + slab(
      ramp_back_bottom_x,
      ramp_front_bottom_x,
      base_left_y,
      base_right_y,
      base_top_z,
      ramp_back_top_x,
      ramp_front_top_x,
      ramp_left_top_y,
      ramp_right_top_y,
      ramp_top_z
    )
    + slab(
      ramp_back_top_x,
      ramp_front_top_x,
      ramp_left_top_y,
      ramp_right_top_y,
      ramp_top_z,
      grill_back_top_x,
      grill_front_top_x,
      grill_left_top_y,
      grill_right_top_y,
      grill_top_z
    )
    + u.rounded_bh_triangle(tri_base, tri_height, tri_length)
      :rotate(0, 90, 0)
      :translate(tri_x, 0, tri_z)
    -- Air scoop on the nose: a shallow tapered box hollowed out.
    + (
      u.rounded_trapezoid(40.0, 55.0, top_outer_length, tri_height)
        :rotate(0, -90, 0)
        :translate(tri_x, 0, top_outer_z)
      - u.rounded_trapezoid(35.0, 48.5, top_inner_length, tri_height)
        :rotate(0, -90, 0)
        :translate(tri_x, 0, top_inner_z)
    )
    + blister:translate(dot_x, dot_y, dot_z)
    + blister:translate(dot_x, -dot_y, dot_z)

  local diamond = u.rounded_diamond(diamond_length, 5.0, vent_height)
    :rotate(0, 90, 0)

  -- Front slot: the outer taper minus a smaller one, so only a thin
  -- rectangular mouth is actually removed.
  local slot = u.rounded_trapezoid(9.0, 20.0, slot_outer_length, vent_height)
    :rotate(0, 90, 0)
    :translate(vent_x, 0, slot_outer_z)
    - u.rounded_trapezoid(3.0, 6.0, slot_inner_length, vent_height)
      :rotate(0, 90, 0)
      :translate(vent_x, 0, slot_inner_z)

  return body
    - diamond:translate(vent_x, diamond_y, diamond_z)
    - diamond:translate(vent_x, -diamond_y, diamond_z)
    - slot
end

--------------------------------------------------------------------------
-- Deck outline
--------------------------------------------------------------------------

-- Tail: a blunt point that widens into the full-width middle section.
local function deck_back()
  local pointed_length = 5.0
  local pointed_x = -M.back_length / 2 + pointed_length / 2

  local base_length = M.back_length - pointed_length
  local base_x = pointed_x + pointed_length / 2 + base_length / 2

  local rounded_length = 43.18
  local rounded_x = base_x + base_length / 2 - rounded_length / 2 + 0.01

  local wedge = u.triangle(pointed_length, M.height, M.back_width / 2)
  local tip_x = pointed_x + pointed_length / 2

  return wedge:rotate(90, 0, 180):translate(tip_x, 0, -M.height / 2)
    + wedge:rotate(-90, 0, 180):translate(tip_x, 0, M.height / 2)
    + cube { { base_length, M.back_width, M.height }, center = true }
      :translate(base_x, 0, 0)
    + u.trapezoid(M.back_width, M.width, rounded_length, M.height)
      :rotate(0, 0, 180)
      :translate(rounded_x, 0, 0)
end

-- Middle: full width at the back, tapering twice on the way forward.
local function deck_middle()
  local back_trap_length = 194.0
  local back_trap_x = -middle_length / 2 + back_trap_length / 2

  local front_trap_length = middle_length - back_trap_length
  local front_trap_width = 108.62
  local front_trap_x = back_trap_x + back_trap_length / 2 + front_trap_length / 2

  return u.trapezoid(front_trap_width, M.width, back_trap_length, M.height)
    :translate(back_trap_x, 0, 0)
    + u.trapezoid(
      M.front_width,
      front_trap_width,
      front_trap_length,
      M.height
    )
      -- A hair of overlap so the two sections weld cleanly.
      :scale(1.001, 1.0, 1.0)
      :translate(front_trap_x, 0, 0)
end

-- Nose: the mirror of the tail.
local function deck_front()
  local pointed_length = 5.0
  local pointed_x = M.front_length / 2 - pointed_length / 2

  local base_length = M.front_length - pointed_length
  local base_x = pointed_x - pointed_length / 2 - base_length / 2

  local wedge = u.triangle(pointed_length, M.height, M.front_width / 2)
  local tip_x = pointed_x - pointed_length / 2

  return wedge:rotate(90, 0, 0):translate(tip_x, 0, -M.height / 2)
    + wedge:rotate(-90, 0, 0):translate(tip_x, 0, M.height / 2)
    + cube { { base_length, M.front_width, M.height }, center = true }
      :translate(base_x, 0, 0)
end

local function deck_base()
  return deck_back():translate(M.back_x, 0, 0)
    + deck_middle():translate(middle_x, 0, 0)
    + deck_front():translate(M.front_x, 0, 0)
end

-- Rounds the deck outline by 2 mm. Eroding with a square and dilating with
-- a cylinder is how the original fillets the corners: the erosion is done
-- by Minkowski-growing the *negative* space, which is why the oversized
-- cutout box appears here.
local function deck_rounded()
  local rounder_radius = 2.0
  local base = deck_base()

  local negative = (
    cube {
      { 2 * M.length, 2 * M.width, M.height - 0.01 },
      center = true,
    } - base
  ):minkowski(cube {
    { 2 * rounder_radius, 2 * rounder_radius, M.height },
    center = true,
  })

  return (base - negative)
    :minkowski(cylinder {
      h = M.height,
      r = rounder_radius,
      center = true,
      fn = 100,
    })
    -- Minkowski doubled the thickness; halve it back.
    :scale(1.0, 1.0, 0.5)
end

--------------------------------------------------------------------------
-- Bolt pattern
--
-- Every hole is the same countersunk M3, dropped in from above.
--------------------------------------------------------------------------

local holes = {
  -- Rear bumper
  { -M.length / 2 + 5.2, 0.0 },
  { -M.length / 2 + 8.1, 21.0 },
  { -M.length / 2 + 8.1, -21.0 },
  -- Rear suspension
  { -M.length / 2 + 52.0, 16.25 },
  { -M.length / 2 + 52.0, -16.25 },
  { -M.length / 2 + 25.5, 16.25 },
  { -M.length / 2 + 25.5, -16.25 },
  -- Motor cover
  { -M.length / 2 + 66.0, 22.0 },
  { -M.length / 2 + 66.0, -22.0 },
  -- Motor mount, offset 8 mm off centre to clear the drive shaft
  { -M.length / 2 + 100.0, -8.0 },
  { -M.length / 2 + 93.0, -8.0 + 36.25 },
  { -M.length / 2 + 93.0, -8.0 - 36.25 },
  -- Crossbar
  { -M.length / 2 + 200.5, -8.2 },
  { -M.length / 2 + 184.5, -8.2 },
  -- Steering servo
  { -M.length / 2 + 271.0, -21.5 },
  { -M.length / 2 + 223.0, -21.5 },
  -- Front suspension
  { M.length / 2 - 27.0, 16.0 },
  { M.length / 2 - 27.0, -16.0 },
  { M.length / 2 - 52.0, 16.0 },
  { M.length / 2 - 52.0, -16.0 },
  -- Front bumper
  { M.length / 2 - 5.0, 0.0 },
  { M.length / 2 - 8.0, 21.5 },
  { M.length / 2 - 8.0, -21.5 },
}

--------------------------------------------------------------------------
-- Platform
--------------------------------------------------------------------------

function M.platform()
  local deck = deck_rounded()

  -- The raised lip: the deck outline shrunk slightly and subtracted from
  -- itself, stretched to wall height, then opened up at both ends.
  local wall = (
    (
      deck_rounded() - deck_rounded():scale(0.99, 0.95, 1.0)
    )
      :scale(1.0, 1.0, wall_height / M.height)
      :translate(0, 0, M.height / 2 + wall_height / 2)
    - cube {
      { M.front_length, M.front_width + 0.01, wall_height },
      center = true,
    }:translate(M.front_x, 0, M.height / 2 + wall_height / 2)
    - cube {
      { M.back_length, M.back_width + 0.01, wall_height },
      center = true,
    }:translate(M.back_x, 0, M.height / 2 + wall_height / 2)
  )

  local connector = u.cyl(
    suspension_connector_height,
    M.suspension_connector_radius
  ):rotate(0, 90, 0)

  local solid = deck
    + wall
    + connector:translate(
      M.back_suspension_connector_x,
      M.back_suspension_connector_left_y,
      M.back_suspension_connector_z
    )
    + connector:translate(
      M.back_suspension_connector_x,
      M.back_suspension_connector_right_y,
      M.back_suspension_connector_z
    )
    + connector:translate(
      M.front_suspension_connector_x,
      M.front_width / 2 - 2.0,
      M.back_suspension_connector_z
    )
    + connector:translate(
      M.front_suspension_connector_x,
      -M.front_width / 2 + 2.0,
      M.back_suspension_connector_z
    )
    + front_bumper():translate(
      M.length / 2 - 8.0 - u.m3_screw_head_radius,
      0,
      -M.height / 2 + 2
    )

  -- 1.001 in Z so the countersink breaks cleanly through both faces.
  local screw = u.m3_flathead_screw(M.height)
    :scale(1.0, 1.0, 1.001)
    :rotate(180, 0, 0)

  for _, hole in ipairs(holes) do
    solid = solid - screw:translate(hole[1], hole[2], 0)
  end

  return solid
end

return M
