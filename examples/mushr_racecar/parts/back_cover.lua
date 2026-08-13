-- Rear bodywork: the flat top panel that carries the lidar and the Jetson,
-- and the two sloping side panels that close the rear bay.
--
-- Ported from v3/scad/racecar_back_cover_top.scad,
-- racecar_back_cover_left_side.scad and racecar_back_cover_right_side.scad.
--
-- The original insets a DXF logo into each side panel; those are dropped.

local u = require("parts.utils")
local foundation = require("parts.back_foundation")

local M = {}

M.length = foundation.length
M.width = 100.0
M.height = 10.0

M.x = 0.0
M.y = 0.0
M.z = foundation.height / 2 + M.height / 2 + 65.0

-- The two rails along the underside that the side panels bolt to.
local rail_width = 15.0
local rail_height = 3.0
local rail_y = M.width / 2 - rail_width / 2
local rail_z = -M.height / 2 + rail_height / 2

local jetson_access_length = u.m2_5_nut_diameter / 2
local jetson_screw_height = 8.0
local jetson_front_x = -M.length / 2 + 80 - 4.125
local jetson_back_x = jetson_front_x - 57.75
local jetson_left_y = 85.75 / 2 - 3.0
local jetson_right_y = jetson_left_y - 85.75
local jetson_screw_z = -M.height / 2 + jetson_screw_height / 2

local connector_screw_height = M.height - rail_height - 0.75
local connector_front_x = M.length / 2 - u.m2_5_nut_diameter / 2 - u.wall_thickness
local connector_back_left_x = -M.length / 2
  + u.m2_5_nut_diameter / 2
  + u.wall_thickness
local connector_back_right_x = -M.length / 2 + 30.0
local connector_left_y = M.width / 2
  - u.m2_5_screw_shaft_radius
  - u.wall_thickness
local connector_right_y = -M.width / 2
  + u.m2_5_screw_shaft_radius
  + u.wall_thickness
local connector_screw_z = -M.height / 2
  + rail_height
  + connector_screw_height / 2

-- Feet of the YDLidar, which sits on top. The lidar is positioned by
-- lining its own feet up with these, so they are public.
local laser_leg_radius = 5.75 / 2
local laser_leg_height = 5.0
M.laser_leg_front_x = M.length / 2 - 10.0
M.laser_leg_front_y = 31.0
local laser_leg_front_x = M.laser_leg_front_x
local laser_leg_back_x = laser_leg_front_x - 57.0
local laser_leg_front_y = M.laser_leg_front_y
local laser_leg_back_y = 25.0
local laser_leg_z = M.height / 2 - laser_leg_height / 2

--------------------------------------------------------------------------
-- Top panel
--------------------------------------------------------------------------

-- One lidar foot: a counterbore, the countersunk screw below it, and a
-- clearance shaft the rest of the way through the panel.
local function laser_mount()
  local screw_head_radius = 4.0 / 2
  local screw_height = 3.0
  local screw_z = laser_leg_z - laser_leg_height / 2 - screw_height / 2

  local extra_height = M.height - laser_leg_height - screw_height
  local extra_z = screw_z - screw_height / 2 - extra_height / 2

  return u.cyl(laser_leg_height, laser_leg_radius)
    :scale(1, 1, 1.001)
    :translate(0, 0, laser_leg_z)
    + u.flathead_screw(screw_height, 7.64 - 6.5, screw_head_radius, 2.35 / 2)
      :rotate(0, 180, 0)
      :translate(0, 0, screw_z)
    + u.cyl(extra_height, screw_head_radius)
      :scale(1, 1, 1.001)
      :translate(0, 0, extra_z)
end

-- A through-hole for the lidar's own PCB, with a nut trap beneath.
local function board_screw()
  local screw_height = 3.0
  local screw_z = M.height / 2 - screw_height / 2
  local nut_height = M.height - screw_height

  return u.cyl(screw_height, u.m2_5_screw_shaft_radius)
    :translate(0, 0, screw_z)
    + u.hexagon(nut_height, u.m2_5_nut_diameter)
      :translate(0, 0, screw_z - screw_height / 2 - nut_height / 2)
end

-- The slot each side rail drops into, plus the channel its captive nut
-- slides along. `sign` is +1 for the left edge, -1 for the right; the
-- channel always runs outwards from the screw to the nearest edge.
local function connector_pocket(sign, y)
  local run = M.width / 2 - abs(y)

  return u.m2_5_nut_insert(connector_screw_height):rotate(0, 0, 30)
    + cube { { u.m2_5_nut_diameter, run, u.m2_5_nut_height }, center = true }
      :translate(
        0,
        sign * run / 2,
        -connector_screw_height / 2 + u.wall_thickness + u.m2_5_nut_height / 2
      )
end

function M.top()
  local laser_cable_length = 20
  local laser_cable_width = 12
  local laser_cable_x = laser_leg_front_x
    - 4.0 / 2
    - laser_cable_length / 2
    - u.wall_thickness
  local laser_cable_y = laser_leg_front_y - laser_cable_width / 2

  local board_front_x = (laser_leg_front_x + laser_leg_back_x) / 2 + 26.0 / 2
  local board_back_x = board_front_x - 26.0
  local board_right_y = -laser_leg_front_y + 5.0
  local board_left_y = board_right_y + 17.0

  local imu_screw_height = M.height - u.wall_thickness
  local imu_front_x = M.length / 2 - u.wall_thickness
  local imu_back_x = imu_front_x - 26.67
  local imu_y = 26.67 / 2
  local imu_screw_z = -M.height / 2 + imu_screw_height / 2

  local mount = laser_mount()
  local board = board_screw()
  local jetson_insert = u.m2_5_nut_insert_with_access(
    jetson_access_length,
    jetson_screw_height
  )
  local imu_insert = u.m2_5_nut_insert_with_access(
    u.m2_5_nut_diameter / 2,
    imu_screw_height
  )

  local solid = cube { { M.length, M.width, M.height }, center = true }
    - mount:translate(laser_leg_front_x, laser_leg_front_y, 0)
    - mount:translate(laser_leg_front_x, -laser_leg_front_y, 0)
    - mount:translate(laser_leg_back_x, laser_leg_back_y, 0)
    - mount:translate(laser_leg_back_x, -laser_leg_back_y, 0)
    - board:translate(board_front_x, board_left_y, 0)
    - board:translate(board_front_x, board_right_y, 0)
    - board:translate(board_back_x, board_left_y, 0)
    - board:translate(board_back_x, board_right_y, 0)
    - cube {
      { laser_cable_length, laser_cable_width, M.height },
      center = true,
    }:translate(laser_cable_x, laser_cable_y, 0)

  for _, x in ipairs { imu_front_x, imu_back_x } do
    solid = solid
      - imu_insert:translate(x, imu_y, imu_screw_z)
      - imu_insert:translate(x, -imu_y, imu_screw_z)
  end

  -- The two rails are removed from the top panel; the side panels carry them.
  solid = solid
    - cube { { M.length, rail_width, rail_height }, center = true }
      :translate(0, rail_y, rail_z)
    - cube { { M.length, rail_width, rail_height }, center = true }
      :translate(0, -rail_y, rail_z)

  for _, x in ipairs { jetson_front_x, jetson_back_x } do
    solid = solid
      - jetson_insert:translate(x, jetson_left_y, jetson_screw_z)
      - jetson_insert:translate(x, jetson_right_y, jetson_screw_z)
  end

  local left_pocket = connector_pocket(1, connector_left_y)
  local right_pocket = connector_pocket(-1, connector_right_y)

  return solid
    - left_pocket:translate(connector_front_x, connector_left_y, connector_screw_z)
    - right_pocket:translate(connector_front_x, connector_right_y, connector_screw_z)
    - left_pocket:translate(
      connector_back_left_x,
      connector_left_y,
      connector_screw_z
    )
    - right_pocket:translate(
      connector_back_right_x,
      connector_right_y,
      connector_screw_z
    )
end

--------------------------------------------------------------------------
-- Side panels
--
-- Each is a sloping sheet spanning from the top panel's edge down to the
-- foundation's side rail, plus the rail it bolts on with.
--------------------------------------------------------------------------

local function side(sign)
  local extra_width = 2.0
  local extra_x = 0.0
  local extra_y = sign * (rail_y + rail_width / 2 + extra_width / 2)

  -- Upper edge: the outer face of the top panel's rail.
  local top_y = extra_y + sign * extra_width / 2
  local top_front_x = M.length / 2
  local top_back_x = -M.length / 2
  local top_high_z = M.z + M.height / 2
  local top_low_z = M.z - M.height / 2

  -- Lower edge: the top of the foundation's side wall.
  local base_outer_y = sign * foundation.width / 2
  local base_inner_y = base_outer_y - sign * u.wall_thickness
  local base_z = foundation.height / 2 + 6.0

  local faces = {
    { 0, 1, 3, 2 },
    { 0, 2, 6, 4 },
    { 1, 5, 7, 3 },
    { 2, 3, 7, 6 },
    { 0, 4, 5, 1 },
    { 4, 6, 7, 5 },
  }

  -- As with the front foundation's kerbs, the right-hand panel is the
  -- mirror image and needs its winding reversed to stay solid.
  if sign < 0 then
    local flipped = {}
    for i, face in ipairs(faces) do
      flipped[i] = { face[4], face[3], face[2], face[1] }
    end
    faces = flipped
  end

  local sheet = polyhedron {
    points = {
      { top_front_x, top_y, top_high_z }, -- 0
      { top_front_x, top_y, top_low_z }, -- 1
      { top_back_x, top_y, top_high_z }, -- 2
      { top_back_x, top_y, top_low_z }, -- 3
      { foundation.length / 2, base_outer_y, base_z }, -- 4
      { foundation.length / 2, base_inner_y, base_z }, -- 5
      { -foundation.length / 2, base_outer_y, base_z }, -- 6
      { -foundation.length / 2, base_inner_y, base_z }, -- 7
    },
    faces = faces,
  }

  -- The rail that mates into the top panel, and the fillet beside it.
  local rail = cube { { M.length, rail_width, rail_height }, center = true }
    :translate(0, sign * rail_y, M.z + rail_z)
  local fillet = cube { { M.length, extra_width, M.height }, center = true }
    :translate(extra_x, extra_y, M.z)

  local jetson_insert = u.m2_5_nut_insert_with_access(
    jetson_access_length,
    jetson_screw_height
  )
  local jetson_y = sign > 0 and jetson_left_y or jetson_right_y
  local connector_y = sign > 0 and connector_left_y or connector_right_y
  local connector_back_x = sign > 0 and connector_back_left_x
    or connector_back_right_x

  rail = rail
    - jetson_insert:translate(jetson_front_x, jetson_y, M.z + jetson_screw_z)
    - jetson_insert:translate(jetson_back_x, jetson_y, M.z + jetson_screw_z)
    - u.cyl(rail_height, u.m2_5_screw_shaft_radius)
      :translate(connector_front_x, connector_y, M.z + rail_z)
    - u.cyl(rail_height, u.m2_5_screw_shaft_radius)
      :translate(connector_back_x, connector_y, M.z + rail_z)

  -- The lip that screws down onto the foundation's side wall.
  local wall_y = sign
    * (foundation.side_wall_left_y + 5.0 / 2 + u.wall_thickness / 2)
  local wall_z = foundation.height / 2 + 6.0 / 2
  local wall = cube {
    { foundation.length, u.wall_thickness, 6.0 },
    center = true,
  }:translate(0, wall_y, wall_z)

  local screw = u.m2_5_flathead_screw(u.wall_thickness)
    :scale(1, 1.001, 1)
    :rotate(-90 * sign, 0, 0)
  local screw_front_x = foundation.length / 2
    - u.m2_5_screw_head_radius
    - u.wall_thickness
  local screw_back_x = -foundation.length / 2
    + u.m2_5_screw_head_radius
    + u.wall_thickness
    + 12.0

  return sheet
    + rail
    + fillet
    + wall
    - screw:translate(screw_front_x, wall_y, wall_z)
    - screw:translate(screw_back_x, wall_y, wall_z)
end

function M.left_side()
  return side(1)
end

function M.right_side()
  return side(-1)
end

return M
