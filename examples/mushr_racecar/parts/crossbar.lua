-- The crossbar: the spine that runs the length of the car and carries the
-- foundations, plus the two feet that bolt it to the chassis plate.
--
-- Ported from v3/scad/racecar_crossbar_body.scad,
-- racecar_crossbar_upper_support.scad and
-- racecar_crossbar_bottom_support.scad.

local u = require("parts.utils")

local M = {}

M.upper_length = 9.4
M.width = 25.0
M.upper_height = 24.5

local upper_screw_x_from_top = 6.67
local upper_screw_radius = 4.0 / 2
local upper_screw_x = M.upper_length / 2 - upper_screw_x_from_top
local upper_screw_y = 16 / 2

M.bottom_length = 7.25
M.bottom_height = M.upper_height

local bottom_screw_x = 0.0
local bottom_screw_y = 17.0 / 2

M.length = 179 + M.upper_length + M.bottom_length
M.height = 2.5

-- The two bolts that hold a foundation down onto the bar. Both foundations
-- are positioned from these, so they are public.
M.body_screw_radius = upper_screw_radius
M.body_screw_front_x = -M.length / 2 + 102.0
M.body_screw_back_x = -M.length / 2 + 87.0
M.body_screw_y = -M.width / 2 + 4.0

M.bottom_screw_x = bottom_screw_x
M.bottom_screw_y = bottom_screw_y
M.upper_screw_x = upper_screw_x
M.upper_screw_y = upper_screw_y

-- Where the two feet sit relative to the bar.
M.bottom_x = -M.length / 2 + M.bottom_length / 2
M.bottom_z = -M.height / 2 - M.bottom_height / 2
M.upper_x = M.length / 2 - M.upper_length / 2
M.upper_z = -M.height / 2 - M.upper_height / 2

--------------------------------------------------------------------------
-- Feet
--------------------------------------------------------------------------

-- Front foot: a block with a wedge-shaped nose, a locating pip on top and
-- a matching socket underneath so a stack of them keys together.
function M.upper_support(height, draw_pip)
  height = height or M.upper_height
  if draw_pip == nil then
    draw_pip = true
  end

  local head_length = 4.75
  local head_width = M.width / 2
  local head_x = -head_length / 2 + M.upper_length / 2

  local base_length = M.upper_length - head_length
  local base_x = base_length / 2 - M.upper_length / 2

  local pip_height = 3.0
  local pip_x = M.upper_length / 2 - 4.5
  local pip_z = pip_height / 2 + height / 2

  local socket_height = 3.5
  local socket_z = socket_height / 2 - height / 2

  local wedge = u.triangle(head_length, height, head_width)
  local nose_x = head_x - head_length / 2

  local solid = wedge:rotate(90, 0, 0):translate(nose_x, 0, -height / 2)
    + wedge:rotate(-90, 0, 0):translate(nose_x, 0, height / 2)
    + cube { { base_length, M.width, height }, center = true }
      :translate(base_x, 0, 0)

  if draw_pip then
    solid = solid + u.cyl(pip_height, 1.5):translate(pip_x, 0, pip_z)
  end

  local screw = u.cyl(height, upper_screw_radius)

  return solid
    - u.cyl(socket_height, 1.75):translate(pip_x, 0, socket_z)
    - screw:translate(upper_screw_x, upper_screw_y, 0)
    - screw:translate(upper_screw_x, -upper_screw_y, 0)
end

-- Rear foot: a plain block with the same bolt pattern.
function M.bottom_support(height)
  height = height or M.bottom_height

  local screw = u.cyl(height, upper_screw_radius)

  return cube { { M.bottom_length, M.width, height }, center = true }
    - screw:translate(bottom_screw_x, bottom_screw_y, 0)
    - screw:translate(bottom_screw_x, -bottom_screw_y, 0)
end

--------------------------------------------------------------------------
-- Crossbar
--------------------------------------------------------------------------

function M.body()
  local body_length = M.length - M.upper_length - M.bottom_length
  local body_x = M.upper_x - M.upper_length / 2 - body_length / 2
  local back_x = body_x - body_length / 2 - M.bottom_length / 2

  local screw = u.cyl(M.height, upper_screw_radius)

  return M.upper_support(M.height, false):translate(M.upper_x, 0, 0)
    + cube { { body_length, M.width, M.height }, center = true }
      :translate(body_x, 0, 0)
    + M.bottom_support(M.height):translate(back_x, 0, 0)
    - screw:translate(M.body_screw_front_x, M.body_screw_y, 0)
    - screw:translate(M.body_screw_back_x, M.body_screw_y, 0)
end

return M
