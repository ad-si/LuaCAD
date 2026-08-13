-- The pillars that stand the electronics foundations off the crossbar.
--
-- Ported from v3/scad/racecar_foundation_support.scad and its three
-- callers (upper left, lower left, lower right).
--
-- All three are the same block at different sizes: a post with a captive
-- nut pocket at each end and a slot to slide the nut in through.

local u = require("parts.utils")
local crossbar = require("parts.crossbar")

local M = {}

M.screw_from_edge = 5.5

function M.support(screw_height, length, width, height)
  screw_height = screw_height or 10
  length = length or 20
  width = width or 10
  height = height or 20

  local screw_x = length / 2 - M.screw_from_edge
  local screw_z = height / 2 - screw_height / 2

  local access_length = length / 2 - screw_x
  local access_z = screw_z
    + screw_height / 2
    - u.wall_thickness
    - u.m2_5_nut_height / 2

  local insert = u.m2_5_nut_insert(screw_height):rotate(180, 0, 0)
  local access = cube {
    { access_length, u.m2_5_nut_diameter, u.m2_5_nut_height },
    center = true,
  }

  return cube { { length, width, height }, center = true }
    - insert:translate(screw_x, 0, screw_z + 0.01)
    - insert:translate(-screw_x, 0, screw_z + 0.01)
    - access:translate(length / 2 - access_length / 2, 0, access_z)
    - access:translate(-length / 2 + access_length / 2, 0, access_z)
end

--------------------------------------------------------------------------
-- The three fitted instances, with where each sits on the crossbar
--------------------------------------------------------------------------

M.upper_left = {
  length = 12.5,
  width = 40.0,
  height = 28.87 + crossbar.upper_height,
}
M.upper_left.x = crossbar.length / 2 - M.upper_left.length / 2 - 13.3
M.upper_left.y = crossbar.width / 2 + M.upper_left.width / 2 + 0.1
M.upper_left.z = crossbar.height / 2 - M.upper_left.height / 2

M.lower_left = {
  length = 23,
  width = 20.0,
  height = 28.87 + crossbar.upper_height,
}
M.lower_left.x = -crossbar.length / 2 - M.lower_left.length / 2 + 13.5
M.lower_left.y = crossbar.width / 2 + M.lower_left.width / 2 + 22.0
M.lower_left.z = crossbar.height / 2 - M.lower_left.height / 2

-- The right-hand pillar is sized to the servo cage it stands beside.
M.lower_right = { length = 60, width = 9 }

function M.upper_left_support()
  -- Turned across the car, so length and width swap.
  return M.support(
    10.0,
    M.upper_left.width,
    M.upper_left.length,
    M.upper_left.height
  ):rotate(0, 0, 90)
end

function M.lower_left_support()
  return M.support(
    10.0,
    M.lower_left.length,
    M.lower_left.width,
    M.lower_left.height
  )
end

function M.lower_right_support(height)
  return M.support(10.0, M.lower_right.length, M.lower_right.width, height)
end

return M
