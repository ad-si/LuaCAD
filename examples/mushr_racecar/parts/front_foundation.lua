-- The front electronics tray: a tapered deck with kerbed walls, the front
-- battery bay, and bolt patterns for three off-the-shelf power boards.
--
-- Ported from v3/scad/racecar_front_foundation.scad.

local u = require("parts.utils")
local crossbar = require("parts.crossbar")
local support = require("parts.foundation_support")
local back = require("parts.back_foundation")

local M = {}

M.length = 110.0
M.width = 130.0
M.height = 5.0

M.x = M.length / 2
  + crossbar.body_screw_front_x
  - (crossbar.body_screw_front_x - crossbar.body_screw_back_x) / 2
M.y = 0.0
M.z = M.height / 2 + crossbar.height / 2

-- The deck is a rectangle at the back and a taper at the front.
local base_length = (crossbar.upper_screw_x + crossbar.upper_x)
  - (crossbar.body_screw_front_x + crossbar.body_screw_back_x) / 2
  - 10
local base_x = -M.length / 2 + base_length / 2

local head_length = M.length - base_length
local head_width = 60.0
local head_x = M.length / 2 - head_length / 2

-- Corners of the tapered nose, used to lay the angled walls along it.
local head_back_x = M.length / 2 - head_length
local head_back_y = M.width / 2
local head_front_x = M.length / 2
local head_front_y = head_width / 2

local wall_thickness = 5.0
local wall_height = 6.0
local wall_bottom_z = M.height / 2
local wall_top_z = wall_bottom_z + wall_height

-- The angled wall follows the taper, inset by one wall thickness.
local wall_back_outer_y = head_back_y - u.wall_thickness
local wall_front_outer_x = head_front_x - u.wall_thickness
local wall_front_outer_y = wall_back_outer_y
  + (wall_front_outer_x - head_back_x)
    * (head_front_y - head_back_y)
    / (head_front_x - head_back_x)

local side_wall_length = M.length - head_length
local side_wall_x = head_x - head_length / 2 - side_wall_length / 2
local side_wall_left_y = M.width / 2 - u.wall_thickness - wall_thickness / 2
local side_wall_right_y = -M.width / 2 + u.wall_thickness + wall_thickness / 2
local side_wall_z = M.height / 2 + wall_height / 2

local side_screw_front_x = side_wall_x
  + side_wall_length / 2
  - u.m2_5_screw_head_radius
  - u.wall_thickness
local side_screw_back_x = side_wall_x
  - side_wall_length / 2
  + u.m2_5_screw_head_radius
  + u.wall_thickness

local front_wall_screw_x = head_x
  + head_length / 2
  - u.wall_thickness
  - wall_thickness / 2
local front_wall_screw_z = M.height / 2 + wall_height / 2

-- The front battery bay mirrors the rear one, 137.5 mm further forward.
local battery_front_length = u.wall_thickness
local battery_front_width = 2 * 48.75 + 3 * u.wall_thickness
local battery_wall_height = 5.0
local battery_front_x = -M.x
  + back.x
  + (-back.length / 2 + battery_front_length / 2)
  + battery_front_length / 2
  + battery_front_length / 2
  + 137.5
local battery_wall_z = M.height / 2 + battery_wall_height / 2

local battery_side_length = 10.0
local battery_side_x = battery_front_x
  - battery_front_length / 2
  - battery_side_length / 2
local battery_side_y = 48.75 + u.wall_thickness

local battery_round_length = battery_side_length + battery_front_length
local battery_round_height = 14.0 - battery_wall_height
local battery_round_x = battery_side_x
  - battery_side_length / 2
  + battery_round_length / 2
local battery_round_z = battery_wall_z + battery_wall_height / 2

--------------------------------------------------------------------------

-- One side of the angled front kerb, as a solid with four corners on the
-- floor and the same four repeated at wall height.
--
-- Mirroring across Y reverses which way each face points, so the right-hand
-- wall needs its vertex order reversed. The original reuses one face list
-- for both sides -- OpenSCAD tolerates the inconsistency, but it leaves the
-- mirrored wall inside-out, so here it would subtract instead of add.
local function angled_wall(sign)
  local back_outer_y = sign * wall_back_outer_y
  local back_inner_y = back_outer_y - sign * wall_thickness
  local front_outer_y = sign * wall_front_outer_y
  local front_inner_y = front_outer_y - sign * wall_thickness

  local faces = {
    { 1, 0, 4, 5 },
    { 4, 0, 2, 6 },
    { 4, 6, 7, 5 },
    { 5, 7, 3, 1 },
    { 0, 1, 3, 2 },
    { 7, 6, 2, 3 },
  }

  if sign < 0 then
    local flipped = {}
    for i, face in ipairs(faces) do
      flipped[i] = { face[4], face[3], face[2], face[1] }
    end
    faces = flipped
  end

  return polyhedron {
    points = {
      { head_back_x, back_outer_y, wall_bottom_z }, -- 0
      { head_back_x, back_inner_y, wall_bottom_z }, -- 1
      { wall_front_outer_x, front_outer_y, wall_bottom_z }, -- 2
      { wall_front_outer_x, front_inner_y, wall_bottom_z }, -- 3
      { head_back_x, back_outer_y, wall_top_z }, -- 4
      { head_back_x, back_inner_y, wall_top_z }, -- 5
      { wall_front_outer_x, front_outer_y, wall_top_z }, -- 6
      { wall_front_outer_x, front_inner_y, wall_top_z }, -- 7
    },
    faces = faces,
  }
end

local function wall_cap()
  return (
    u.cyl(1.0, 0.5)
    - cube { { 1.0, 0.5, 1.0 }, center = true }:translate(0, -0.25, 0)
  )
    :rotate(90, 0, 0)
    :scale(battery_round_length, u.wall_thickness, 2 * battery_round_height)
end

-- A through-hole with a hexagonal nut trap on its underside. The three
-- power boards all mount this way.
local function board_screw()
  return u.cyl(M.height, u.m2_5_screw_shaft_radius)
    + u.hexagon(u.m2_5_nut_height, u.m2_5_nut_diameter)
      :translate(0, 0, -M.height / 2 + u.m2_5_nut_height / 2)
end

function M.front_foundation()
  local crossbar_body_screw_x = -M.x + crossbar.body_screw_front_x

  local crossbar_top_screw_x = -M.x + crossbar.upper_x + crossbar.upper_screw_x
  local crossbar_top_screw_y = crossbar.upper_screw_y

  local ulfs_screw_x = -M.x + support.upper_left.x
  local ulfs_screw_left_y = support.upper_left.y
    + support.upper_left.width / 2
    - support.screw_from_edge
  local ulfs_screw_right_y = support.upper_left.y
    - support.upper_left.width / 2
    + support.screw_from_edge

  -- Mounting patterns for the three power boards that live up front.
  local dzs_back_x = battery_front_x
    + battery_front_length / 2
    + u.m2_5_screw_head_radius
    + u.wall_thickness
    + 7.5
  local dzs_front_x = dzs_back_x + 36.75
  local dzs_y = 53.0 / 2

  local lm2596_back_x = battery_front_x
    + battery_front_length / 2
    + u.m2_5_screw_head_radius
    + u.wall_thickness
  local lm2596_front_x = lm2596_back_x + 31.0
  local lm2596_y = 53.75 / 2

  local drok_back_x = lm2596_back_x
  local drok_front_x = drok_back_x + 38.0
  local drok_y = 69.5 / 2

  local front_hole_width = 10.0
  local front_hole_y = crossbar_top_screw_y
    + u.m3_screw_head_radius
    + u.wall_thickness
    + front_hole_width / 2

  local side_wall = cube {
    { side_wall_length, wall_thickness, wall_height },
    center = true,
  }
  local battery_side_wall = cube {
    { battery_side_length, u.wall_thickness, battery_wall_height },
    center = true,
  }
  local cap = wall_cap()

  local solid = cube { { base_length, M.width, M.height }, center = true }
    :translate(base_x, 0, 0)
    + u.trapezoid(head_width, M.width, head_length, M.height)
      :translate(head_x, 0, 0)
    + angled_wall(1)
    + angled_wall(-1)
    -- The straight kerb across the nose, spanning the two angled ones.
    + cube {
      { wall_thickness, 2 * wall_front_outer_y, wall_height },
      center = true,
    }:translate(front_wall_screw_x, 0, front_wall_screw_z)
    + side_wall:translate(side_wall_x, side_wall_left_y, side_wall_z)
    + side_wall:translate(side_wall_x, side_wall_right_y, side_wall_z)
    + cube {
      { battery_front_length, battery_front_width, battery_wall_height },
      center = true,
    }:translate(battery_front_x, 0, battery_wall_z)
    + battery_side_wall:translate(battery_side_x, battery_side_y, battery_wall_z)
    + battery_side_wall:translate(battery_side_x, -battery_side_y, battery_wall_z)
    + battery_side_wall:translate(battery_side_x, 0, battery_wall_z)
    + cap:translate(battery_round_x, battery_side_y, battery_round_z)
    + cap:translate(battery_round_x, -battery_side_y, battery_round_z)
    + cap:translate(battery_round_x, 0, battery_round_z)
    -- The rear tray's mounting pads reach forward to meet this one.
    + back.attach():translate(back.x - M.x, 0, back.z - M.z)

  local m2_5 = u.m2_5_flathead_screw(M.height)
  local m3 = u.m3_flathead_screw(M.height)
  -- A clearance bore so a driver can reach the bolt through the kerb.
  local driver_access = u.cyl(wall_height, u.m3_screw_head_radius)

  solid = solid
    - m2_5:translate(crossbar_body_screw_x, crossbar.body_screw_y, 0)
    - m3:translate(crossbar_top_screw_x, crossbar_top_screw_y, 0)
    - m3:translate(crossbar_top_screw_x, -crossbar_top_screw_y, 0)
    - driver_access:translate(
      crossbar_top_screw_x,
      crossbar_top_screw_y,
      front_wall_screw_z
    )
    - driver_access:translate(
      crossbar_top_screw_x,
      -crossbar_top_screw_y,
      front_wall_screw_z
    )
    - m2_5:translate(ulfs_screw_x, ulfs_screw_left_y, 0)
    - m2_5:translate(ulfs_screw_x, ulfs_screw_right_y, 0)

  local insert = u.m2_5_nut_insert(wall_thickness):scale(1, 1.001, 1)
  local left_insert = insert:rotate(90, 0, 0)
  local right_insert = insert:rotate(-90, 0, 0)

  solid = solid
    - left_insert:translate(side_screw_front_x, side_wall_left_y, side_wall_z)
    - left_insert:translate(side_screw_back_x, side_wall_left_y, side_wall_z)
    - right_insert:translate(side_screw_front_x, side_wall_right_y, side_wall_z)
    - right_insert:translate(side_screw_back_x, side_wall_right_y, side_wall_z)
    -- Tipped 30 degrees so the nut seats against the sloped kerb.
    - u.m2_5_nut_insert(wall_thickness)
      :rotate(0, -90, 0)
      :rotate(30, 0, 0)
      :scale(1.001, 1, 1)
      :translate(front_wall_screw_x, 0, front_wall_screw_z)

  local screw = board_screw()
  local boards = {
    { dzs_front_x, dzs_y },
    { dzs_back_x, dzs_y },
    { lm2596_front_x, lm2596_y },
    { lm2596_back_x, lm2596_y },
    { drok_front_x, drok_y },
    { drok_back_x, drok_y },
  }

  for _, b in ipairs(boards) do
    solid = solid - screw:translate(b[1], b[2], 0) - screw:translate(b[1], -b[2], 0)
  end

  -- Two cable slots either side of the crossbar bolts.
  local slot = cube {
    { 2 * u.m3_screw_head_radius, front_hole_width, M.height },
    center = true,
  }

  return solid
    - slot:translate(crossbar_top_screw_x, front_hole_y, 0)
    - slot:translate(crossbar_top_screw_x, -front_hole_y, 0)
end

--------------------------------------------------------------------------
-- Edges the bodywork keys off
--
-- The front covers are built as sheets spanning from the rear cover's top
-- panel down to these edges, so every one of them is public.
--------------------------------------------------------------------------

M.head_back_x = head_back_x
M.head_back_y = head_back_y
M.head_front_x = head_front_x
M.head_front_y = head_front_y

-- The kerb is 5 mm thick, distinct from utils' 3 mm general wall.
M.kerb_thickness = wall_thickness
M.wall_height = wall_height
M.wall_bottom_z = wall_bottom_z
M.wall_top_z = wall_top_z
M.wall_front_outer_x = wall_front_outer_x
M.wall_front_outer_y = wall_front_outer_y
-- The kerb's rear end sits on the head's back edge, inset by one wall.
M.wall_back_outer_x = head_back_x
M.wall_back_outer_y = wall_back_outer_y

M.side_wall_x = side_wall_x
M.side_wall_length = side_wall_length
M.side_wall_left_y = side_wall_left_y
M.side_wall_right_y = side_wall_right_y
M.side_wall_z = side_wall_z
M.side_screw_front_x = side_screw_front_x
M.side_screw_back_x = side_screw_back_x

return M
