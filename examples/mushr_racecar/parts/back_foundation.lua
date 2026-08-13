-- The rear electronics tray: a flat deck with side rails, a moulded
-- battery bay, ventilation slots and a grid of zip-tie holes.
--
-- Ported from v3/scad/racecar_back_foundation.scad.

local u = require("parts.utils")
local crossbar = require("parts.crossbar")
local support = require("parts.foundation_support")

local M = {}

M.length = 110
M.width = 130
M.height = 5

-- Straddles the crossbar's two mounting bolts.
M.x = -M.length / 2
  + crossbar.body_screw_front_x
  - (crossbar.body_screw_front_x - crossbar.body_screw_back_x) / 2
M.y = 0.0
M.z = M.height / 2 + crossbar.height / 2

local side_wall_width = 5.0
local side_wall_height = 6.0
M.side_wall_left_y = M.width / 2 - u.wall_thickness - side_wall_width / 2
M.side_wall_right_y = -M.width / 2 + u.wall_thickness + side_wall_width / 2
local side_wall_z = M.height / 2 + side_wall_height / 2

local side_screw_front_x = M.length / 2
  - u.m2_5_screw_head_radius
  - u.wall_thickness
local side_screw_back_x = -M.length / 2
  + u.m2_5_screw_head_radius
  + u.wall_thickness
  + 12.0

local battery_back_length = u.wall_thickness
local battery_back_width = 2 * 48.75 + 3 * u.wall_thickness
local battery_wall_height = 5.0
local battery_back_x = -M.length / 2 + battery_back_length / 2
local battery_wall_z = M.height / 2 + battery_wall_height / 2

local battery_side_length = 10.0
local battery_side_x = battery_back_x
  + battery_back_length / 2
  + battery_side_length / 2
local battery_side_y = 48.75 + u.wall_thickness

local battery_round_length = battery_side_length + battery_back_length
local battery_round_height = 14.0 - battery_wall_height
local battery_round_x = battery_side_x
  + battery_side_length / 2
  - battery_round_length / 2
local battery_round_z = battery_wall_z + battery_wall_height / 2

local attach_length = 10.0
local attach_width = 10.0
local attach_height = M.height / 2
local attach_x = M.length / 2 - attach_length / 2
local attach_left_y = M.side_wall_left_y - side_wall_width / 2 - attach_width / 2
local attach_right_y = M.side_wall_right_y
  + side_wall_width / 2
  + attach_width / 2
local attach_z = M.height / 2 - attach_height / 2

--------------------------------------------------------------------------

-- The rounded cap along the top of each battery wall: a cylinder squashed
-- into an ellipse with its lower half removed.
local function wall_cap()
  return (
    u.cyl(1.0, 0.5)
    - cube { { 1.0, 0.5, 1.0 }, center = true }:translate(0, -0.25, 0)
  )
    :rotate(90, 0, 0)
    :scale(battery_round_length, u.wall_thickness, 2 * battery_round_height)
end

-- The two mounting pads at the front edge, drawn without their bolt holes
-- so they can be subtracted from the deck as a pocket.
local function attach_pads()
  local pad = cube {
    { attach_length, attach_width, attach_height },
    center = true,
  }
  return pad:translate(attach_x, attach_left_y, attach_z)
    + pad:translate(attach_x, attach_right_y, attach_z)
end

function M.attach()
  local screw = u.m2_5_flathead_screw(attach_height)

  return attach_pads()
    - screw:translate(attach_x, attach_left_y, attach_z)
    - screw:translate(attach_x, attach_right_y, attach_z)
end

function M.back_foundation()
  -- Bolt positions inherited from whatever this foundation sits on, all
  -- expressed relative to the foundation's own centre.
  local crossbar_body_screw_x = -M.x + crossbar.body_screw_back_x
  local crossbar_body_screw_y = -M.y + crossbar.body_screw_y

  local crossbar_bottom_screw_x = -M.x
    + crossbar.bottom_x
    + crossbar.bottom_screw_x
  local crossbar_bottom_screw_y = crossbar.bottom_screw_y

  local llfs_front_x = -M.x
    + support.lower_left.x
    + support.lower_left.length / 2
    - support.screw_from_edge
  local llfs_back_x = -M.x
    + support.lower_left.x
    - support.lower_left.length / 2
    + support.screw_from_edge
  local llfs_y = -M.y + support.lower_left.y

  local lrfs_x = crossbar.body_screw_back_x - support.lower_right.length / 2 - 26.5
  local lrfs_y = crossbar.body_screw_y - support.lower_right.width / 2 - 40.67
  local lrfs_front_x = -M.x
    + lrfs_x
    + support.lower_right.length / 2
    - support.screw_from_edge
  local lrfs_back_x = -M.x
    + lrfs_x
    - support.lower_right.length / 2
    + support.screw_from_edge

  -- The two long ventilation slots, and the zip-tie grid punched through
  -- each of them.
  local cutout_length = M.length - 20.0
  -- Spans from the right-hand crossbar bolt out to the right-hand pillar,
  -- halved. Both slots use that same width, mirrored.
  local cutout_width = (-crossbar_bottom_screw_y - (-M.y + lrfs_y)) / 2
  local cutout_height = 2.0
  local cutout_y = M.width / 4
  local cutout_z = M.height / 2 - cutout_height / 2

  local zip_n, zip_m = 6, 2
  local zip_length, zip_width = 7.5, 2.0
  local zip_x_interval = (cutout_length - zip_n * zip_length) / (zip_n + 1.0)
  local zip_y_interval = (cutout_width - zip_m * zip_width) / (zip_m + 1.0)
  local zip_start_x = -((zip_n - 1) * zip_length) / 2
    - (zip_n / 2.0 - 0.5) * zip_x_interval
  local function zip_start_y(centre)
    return centre
      - ((zip_m - 1) * zip_width) / 2
      - (zip_m / 2.0 - 0.5) * zip_y_interval
  end

  local access_height = battery_round_height + battery_wall_height

  local vesc_width = 12.0
  local vesc_y = M.side_wall_left_y - side_wall_width / 2 - vesc_width / 2

  local side_wall = cube {
    { M.length, side_wall_width, side_wall_height },
    center = true,
  }
  local battery_side_wall = cube {
    { battery_side_length, u.wall_thickness, battery_wall_height },
    center = true,
  }
  local cap = wall_cap()

  local solid = cube { { M.length, M.width, M.height }, center = true }
    + side_wall:translate(0, M.side_wall_left_y, side_wall_z)
    + side_wall:translate(0, M.side_wall_right_y, side_wall_z)
    + cube {
      { battery_back_length, battery_back_width, battery_wall_height },
      center = true,
    }:translate(battery_back_x, 0, battery_wall_z)
    -- Three fore-and-aft walls make two battery bays.
    + battery_side_wall:translate(battery_side_x, battery_side_y, battery_wall_z)
    + battery_side_wall:translate(battery_side_x, -battery_side_y, battery_wall_z)
    + battery_side_wall:translate(battery_side_x, 0, battery_wall_z)
    + cap:translate(battery_round_x, battery_side_y, battery_round_z)
    + cap:translate(battery_round_x, -battery_side_y, battery_round_z)
    + cap:translate(battery_round_x, 0, battery_round_z)

  local m2_5 = u.m2_5_flathead_screw(M.height)
  local m3 = u.m3_flathead_screw(M.height)

  solid = solid
    - m2_5:translate(crossbar_body_screw_x, crossbar_body_screw_y, 0)
    - m3:translate(crossbar_bottom_screw_x, crossbar_bottom_screw_y, 0)
    - m3:translate(crossbar_bottom_screw_x, -crossbar_bottom_screw_y, 0)
    - m2_5:translate(llfs_front_x, llfs_y, 0)
    - m2_5:translate(llfs_back_x, llfs_y, 0)
    - m2_5:translate(lrfs_front_x, -M.y + lrfs_y, 0)
    - m2_5:translate(lrfs_back_x, -M.y + lrfs_y, 0)

  local slot = u.rounded_square(cutout_length, cutout_width, cutout_height)
  local zip = u.rounded_square(zip_length, zip_width, M.height)

  for _, centre in ipairs { cutout_y, -cutout_y } do
    solid = solid - slot:translate(0, centre, cutout_z)
    for i = 0, zip_n - 1 do
      for j = 0, zip_m - 1 do
        solid = solid
          - zip:translate(
            zip_start_x + i * (zip_length + zip_x_interval),
            zip_start_y(centre) + j * (zip_width + zip_y_interval),
            0
          )
      end
    end
  end

  -- Captive nuts in the side rails, entered from outside.
  local insert = u.m2_5_nut_insert(side_wall_width):scale(1, 1.001, 1)
  local left_insert = insert:rotate(90, 0, 0)
  local right_insert = insert:rotate(-90, 0, 0)

  return solid
    - left_insert:translate(side_screw_front_x, M.side_wall_left_y, side_wall_z)
    - left_insert:translate(side_screw_back_x, M.side_wall_left_y, side_wall_z)
    - right_insert:translate(side_screw_front_x, M.side_wall_right_y, side_wall_z)
    - right_insert:translate(side_screw_back_x, M.side_wall_right_y, side_wall_z)
    -- A shaft down through the battery wall to reach the pillar bolt below.
    - u.cyl(access_height, u.m2_5_screw_head_radius)
      :translate(llfs_back_x, llfs_y, M.height / 2 + access_height / 2)
    -- Cable pass-through for the motor controller
    - cube { { 25.0, vesc_width, M.height }, center = true }:translate(0, vesc_y, 0)
    - attach_pads()
    - u.cyl(M.height, u.m2_5_screw_shaft_radius)
      :translate(attach_x, attach_left_y, 0)
    - u.cyl(M.height, u.m2_5_screw_shaft_radius)
      :translate(attach_x, attach_right_y, 0)
end

return M
