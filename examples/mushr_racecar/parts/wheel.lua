-- Wheel, tyre, hub and drive axle.
--
-- Ported from chassis/racecar_chassis_wheel.scad.
--
-- The wheel lies on its side: it is a cylinder swept about the Y axis, so
-- "left" and "right" below are +y and -y.

local u = require("parts.utils")

local M = {}

-- Tyre carcass
local outer_radius = 97.6 / 2
local outer_height = 35.0

-- The dished faces on either side of the carcass
local cap_radius = 88.11 / 2
local cap_height = 3.0
local cap_left_y = outer_height / 2 + cap_height / 2
local cap_right_y = -outer_height / 2 - cap_height / 2

-- Hollow interior
local inner_height = 30.0
local inner_radius = 71 / 2
local inner_y = outer_height / 2 + cap_height - inner_height / 2 + 0.001

-- The six tapered windows around the face
local face_cutout_length = 20.0
local face_cutout_back_width = 17.5
local face_cutout_front_width = 5.0
local face_cutout_height = outer_height + 2 * cap_height - inner_height + 0.001
local face_cutout_y = cap_right_y - cap_height / 2 + face_cutout_height / 2
local face_cutout_z = face_cutout_length / 2 + 11.8

local spoke_cutout_height = face_cutout_height / 2
local spoke_cutout_radius = 15.0 / 2
local spoke_cutout_y = face_cutout_y
  - face_cutout_height / 2
  + spoke_cutout_height / 2
  - 0.01

-- Stub axle
local axle_height = outer_height + 2 * cap_height - face_cutout_height + 3.75
local axle_radius = 14.5 / 2
local axle_y = face_cutout_y + face_cutout_height / 2 + axle_height / 2

-- Drive cup the axle plugs into
local drive_outer_height = 10.5
local drive_outer_radius = 19.6 / 2
local drive_inner_height = drive_outer_height / 2
local drive_inner_radius = 15.5 / 2
local drive_y = axle_y + axle_height / 2 + drive_outer_height / 2

-- Upright carrying the upper suspension link
local upper_suspension_length = 29.0
local upper_suspension_back_width = 17.5
local upper_suspension_front_width = 12.5
local upper_suspension_height = 8.0
local upper_suspension_z = upper_suspension_length / 2 + 2.5

-- Lower pivot
local lower_cube_length = 8.45
local lower_cube_width = drive_outer_height
local lower_cube_height = 7.0
local lower_cube_z = -lower_cube_height / 2 - 6.0

local lower_cyl_height = 15.0
local lower_cyl_radius = 7.0 / 2
local lower_cyl_z = lower_cube_z - lower_cube_height / 2 - 1.0

-- Steering knuckle plate
local outer_link_length = 18.5
local outer_link_front_width = 38.5
local outer_link_back_width = 54.0
local outer_link_height = 4.1
local inner_link_length = 11.67
local inner_link_front_width = 24.45
local inner_link_back_width = 42.0
local link_y = cap_left_y + cap_height / 2 + outer_link_length / 2 - 9.5

-- Where the tyre ends and the hub begins
local tire_cutout_radius = cap_radius - 5.0
local tire_cutout_height = outer_height + 2 * cap_height + 0.001

-- Offsets other modules need to position themselves against the wheel.
M.suspension_link_x_offset = 0.0
M.suspension_link_y_offset = link_y
M.suspension_link_z_offset = 0.0
M.outer_suspension_link_length = outer_link_length
M.outer_suspension_link_back_width = outer_link_back_width
M.outer_suspension_link_height = outer_link_height

-- A cylinder swept about the Y axis, as everything on the wheel is.
local function disc(h, r)
  return cylinder { h = h, r = r, center = true, fn = u.fn }:rotate(90, 0, 0)
end

--------------------------------------------------------------------------
-- Wheel
--------------------------------------------------------------------------

function M.wheel()
  local carcass = disc(outer_height, outer_radius)
    + cylinder {
      h = cap_height,
      r1 = outer_radius,
      r2 = cap_radius,
      center = true,
      fn = u.fn,
    }
      :rotate(-90, 0, 0)
      :translate(0, cap_left_y, 0)
    + cylinder {
      h = cap_height,
      r1 = outer_radius,
      r2 = cap_radius,
      center = true,
      fn = u.fn,
    }
      :rotate(90, 0, 0)
      :translate(0, cap_right_y, 0)

  local solid = carcass - disc(inner_height, inner_radius):translate(0, inner_y, 0)

  -- Six windows at 60 degree spacing around the face
  local window = u.rounded_trapezoid(
    face_cutout_front_width,
    face_cutout_back_width,
    face_cutout_length,
    face_cutout_height
  )
    :rotate(0, 90, 90)
    :translate(0, face_cutout_y, face_cutout_z)

  for i = 0, 5 do
    solid = solid - window:rotate(0, i * 60, 0)
  end

  return solid
    - disc(spoke_cutout_height, spoke_cutout_radius)
      :translate(0, spoke_cutout_y, 0)
end

-- The rubber: everything outside the hub radius.
function M.tire()
  return M.wheel() - disc(tire_cutout_height, tire_cutout_radius)
end

-- The printed centre: everything the tyre is not.
function M.hub()
  return M.wheel() - M.tire()
end

--------------------------------------------------------------------------
-- Axle
--------------------------------------------------------------------------

-- `right_ball_head` puts the link's ball joint on the -y side; pass false
-- for the mirrored corner on the other side of the car.
function M.axle(right_ball_head)
  if right_ball_head == nil then
    right_ball_head = true
  end

  local knuckle = u.rounded_trapezoid(
    outer_link_front_width,
    outer_link_back_width,
    outer_link_length,
    outer_link_height
  ) - u.rounded_trapezoid(
    inner_link_front_width,
    inner_link_back_width,
    inner_link_length,
    outer_link_height
  ):translate(-outer_link_length / 2 + inner_link_length / 2 - 2, 0, 0)

  local ball_y = right_ball_head
      and (-outer_link_back_width / 2 + u.ball_head_width / 2 + 1)
    or (outer_link_back_width / 2 - u.ball_head_width / 2 - 1)

  knuckle = knuckle
    + u.ball_head():translate(
      -outer_link_length / 2 + u.ball_head_length / 2,
      ball_y,
      outer_link_height / 2 + u.ball_head_height / 2
    )

  local upright = disc(drive_outer_height, drive_outer_radius)
      :translate(0, drive_y, 0)
    + u.rounded_trapezoid(
      upper_suspension_front_width,
      upper_suspension_back_width,
      upper_suspension_length,
      upper_suspension_height
    )
      :rotate(0, -90, 90)
      :translate(0, drive_y, upper_suspension_z)
    + cube {
      { lower_cube_length, lower_cube_width, lower_cube_height },
      center = true,
    }:translate(0, drive_y, lower_cube_z)
    + cylinder {
      h = lower_cyl_height,
      r = lower_cyl_radius,
      center = true,
      fn = u.fn,
    }
      :rotate(0, 90, 0)
      :translate(0, drive_y, lower_cyl_z)
    + knuckle:rotate(0, 0, -90):translate(0, link_y, 0)

  -- Bore out the drive cup
  local bore = disc(drive_inner_height, drive_inner_radius):translate(
    0,
    drive_y + drive_outer_height / 2 - drive_inner_height / 2,
    0
  )

  return disc(axle_height, axle_radius):translate(0, axle_y, 0)
    + (upright - bore)
end

return M
