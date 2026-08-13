-- The box the steering servo bolts into, standing on the crossbar.
--
-- Ported from v3/scad/racecar_servo_cage.scad.

local u = require("parts.utils")
local crossbar = require("parts.crossbar")

local M = {}

M.length = 55.67
M.width = 21.67
M.height = 32.67 + crossbar.upper_height

local pip_separation = 47.95
local pip_radius = 1.25
local pip_height = 2.25
local pip_edge_offset = 3.5 + pip_radius
local pip_x = pip_separation / 2
local pip_y = -M.width / 2 + pip_edge_offset
local pip_z = -M.height / 2 - pip_height / 2

local bottom_screw_pip_offset = 8.0
local bottom_screw_height = 15.0
local bottom_screw_y = pip_y + bottom_screw_pip_offset
local bottom_screw_z = -M.height / 2 + bottom_screw_height / 2

-- Where the cage sits relative to the crossbar.
M.x = crossbar.length / -2 + 102.0 + pip_separation / 2 + 22.0
M.y = -crossbar.width / 2
  + 4.0
  - 13.15
  - bottom_screw_pip_offset
  - pip_edge_offset
  + M.width / 2
M.z = crossbar.height / 2 - M.height / 2

function M.servo_cage()
  local hole_height = 21.5
  local hole_z = -M.height / 2 + hole_height / 2

  local bottom_access_length = 2 * pip_x
  local bottom_access_z = bottom_screw_z
    - bottom_screw_height / 2
    + u.wall_thickness
    + u.m3_nut_height / 2

  local face_screw_x = 48.75 / 2
  local face_screw_height = 9
  local face_screw_y = -M.width / 2 + face_screw_height / 2
  local face_screw_bottom_z = -M.height / 2 + 5.85
  local face_screw_top_z = face_screw_bottom_z + 10.0

  local face_access_y = -M.width / 2 + u.wall_thickness + u.m3_nut_height / 2

  local cord_width = M.width - 17.5
  local cord_y = M.width / 2 - cord_width / 2

  local pip = u.cyl(pip_height, pip_radius)
  local bottom_insert = u.m3_nut_insert(bottom_screw_height)
  -- Laid on its side so the nut drops in through the outer face.
  local face_insert = u.m3_nut_insert(face_screw_height):rotate(-90, 0, 0)

  local face_access = cube {
    { 2 * face_screw_x, u.m3_nut_height, u.m3_nut_diameter },
    center = true,
  }

  return cube { { M.length, M.width, M.height }, center = true }
    + pip:translate(pip_x, pip_y, pip_z)
    + pip:translate(-pip_x, pip_y, pip_z)
    -- Hollow out the servo pocket
    - cube { { 42.67, M.width, hole_height }, center = true }
      :translate(0, 0, hole_z)
    - bottom_insert:translate(pip_x, bottom_screw_y, bottom_screw_z)
    - bottom_insert:translate(-pip_x, bottom_screw_y, bottom_screw_z)
    - cube {
      { bottom_access_length, u.m3_nut_diameter, u.m3_nut_height },
      center = true,
    }:translate(0, bottom_screw_y, bottom_access_z)
    - face_insert:translate(face_screw_x, face_screw_y, face_screw_bottom_z)
    - face_insert:translate(-face_screw_x, face_screw_y, face_screw_bottom_z)
    - face_insert:translate(face_screw_x, face_screw_y, face_screw_top_z)
    - face_insert:translate(-face_screw_x, face_screw_y, face_screw_top_z)
    - face_access:translate(0, face_access_y, face_screw_top_z)
    - face_access:translate(0, face_access_y, face_screw_bottom_z)
    -- Slot down the back for the servo lead
    - cube { { M.length, cord_width, M.height }, center = true }
      :translate(0, cord_y, 0)
end

return M
