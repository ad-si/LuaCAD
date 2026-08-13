-- Gearbox housing, front steering rack and the suspension arms.
--
-- Ported from chassis/racecar_chassis_gearbox.scad.
--
-- One gearbox sits at each end of the platform; the front one is the same
-- part rotated 180 degrees, with the steering rack bolted to it.

local u = require("parts.utils")

local M = {}

--------------------------------------------------------------------------
-- Dimensions
--
-- Other modules bolt onto the gearbox, so these stay public.
--------------------------------------------------------------------------

M.base_length = 32.38
M.base_width = 30.5
M.base_height = 25.75
M.base_z = M.base_height / 2

local box_radius = 42.0 / 2
local box_height = 24.5
local box_z = M.base_z - M.base_height / 2 + box_radius + 2.0

local column_radius = 6.37 / 2
M.column_front_x = M.base_length / 2 - column_radius
M.column_back_x = -M.base_length / 2 + column_radius
M.column_left_y = M.base_width / 2
M.column_right_y = -M.base_width / 2

local front_connect_height = 10
local front_connect_back_radius = 22 / 2
local front_connect_front_radius = 16.0 / 2
local front_connect_x = box_radius + 2.0

local wheel_slot_outer_radius = 11.17 / 2
local wheel_slot_inner_radius = 6.51 / 2
local wheel_slot_cutout_height = 3.0
local wheel_slot_cutout_width = 8.5
local wheel_slot_height = 13.0
M.wheel_slot_left_y = M.base_width / 2 + wheel_slot_height / 2
M.wheel_slot_right_y = -M.base_width / 2 - wheel_slot_height / 2
M.wheel_slot_z = M.base_z + 2.0

local upper_suspension_radius = 10.0 / 2
local upper_suspension_height = 8.55
local upper_suspension_x = upper_suspension_height / 2
M.upper_suspension_left_y = M.column_left_y + column_radius / 2
M.upper_suspension_right_y = M.column_right_y - column_radius / 2
M.upper_suspension_z = M.base_z + M.base_height / 2 + 10.0

local spring_support_length = 9.25
local spring_support_back_width = 25
local spring_support_front_width = M.upper_suspension_left_y
  - M.upper_suspension_right_y
local spring_support_height = 11.59
local spring_support_radius = 7.5 / 2
local spring_support_x = upper_suspension_x
  - upper_suspension_height / 2
  + 3.5
  - spring_support_height / 2
M.spring_support_z = M.upper_suspension_z
  + upper_suspension_radius
  + spring_support_length / 2
M.spring_support_length = spring_support_length

local spring_support_cutout_length = 7.7
local spring_support_cutout_back_width = 16.5
local spring_support_cutout_front_width = 8.0
local spring_support_cutout_height = spring_support_height - 3.0
local spring_support_cutout_x = spring_support_x
  + spring_support_height / 2
  - spring_support_cutout_height / 2
local spring_support_cutout_z = M.spring_support_z
  + spring_support_length / 2
  - spring_support_cutout_length / 2
  + spring_support_radius

-- Suspension arm
local arm_length = 24.67
local arm_width = 11.9
local arm_height = 4.85

M.arm_length = arm_length

-- Spring plate. The plate itself is left off the assembled car, but the
-- shock towers are positioned against these offsets.
M.plate_base_length = 10.0
M.plate_base_height = spring_support_height - spring_support_cutout_height

local plate_lower_trap_length = 6.0
local plate_lower_trap_back_width = 59.5
local plate_lower_trap_front_width = spring_support_back_width
local plate_lower_trap_x = M.plate_base_length / 2 + plate_lower_trap_length / 2

M.plate_upper_trap_length = 11.0
M.plate_upper_trap_back_width = plate_lower_trap_back_width
M.plate_upper_trap_front_width = 38.0
M.plate_upper_trap_height = M.plate_base_height
M.plate_upper_trap_x = plate_lower_trap_x
  + plate_lower_trap_length / 2
  + M.plate_upper_trap_length / 2

M.plate_upper_cutout_trap_length = 10.0
M.plate_upper_cutout_trap_back_width = 32.0
M.plate_upper_cutout_trap_front_width = 16.0
local plate_upper_cutout_trap_x = M.plate_upper_trap_x
  + M.plate_upper_trap_length / 2
  - M.plate_upper_cutout_trap_length / 2

--------------------------------------------------------------------------
-- Gearbox
--------------------------------------------------------------------------

-- The bearing pocket the wheel shaft passes through: a ring with a slot
-- cut across it at 45 degrees so the shaft can be clipped in.
local function wheel_slot(left)
  local slot_y = left and (wheel_slot_height / 2 - wheel_slot_cutout_width / 2)
    or (-wheel_slot_height / 2 + wheel_slot_cutout_width / 2)

  return u.cyl(wheel_slot_height, wheel_slot_outer_radius):rotate(90, 0, 0)
    - u.cyl(wheel_slot_height, wheel_slot_inner_radius):rotate(90, 0, 0)
    - cube {
      { 2 * wheel_slot_outer_radius, wheel_slot_cutout_width, wheel_slot_cutout_height },
      center = true,
    }
      :translate(0, slot_y, 0)
      :rotate(0, 45, 0)
end

function M.gearbox()
  local column = u.cyl(M.base_height, column_radius)

  local body = cube {
    { M.base_length, M.base_width, M.base_height },
    center = true,
  }:translate(0, 0, M.base_z)
    + u.cyl(box_height, box_radius):rotate(90, 0, 0):translate(0, 0, box_z)
    + column:translate(M.column_front_x, M.column_left_y, M.base_z)
    + column:translate(M.column_front_x, M.column_right_y, M.base_z)
    + column:translate(M.column_back_x, M.column_left_y, M.base_z)
    + column:translate(M.column_back_x, M.column_right_y, M.base_z)
    + u.cyl(
      front_connect_height,
      front_connect_back_radius,
      front_connect_front_radius
    )
      :rotate(0, 90, 0)
      :translate(front_connect_x, 0, box_z)
    + wheel_slot(true):translate(0, M.wheel_slot_left_y, M.wheel_slot_z)
    + wheel_slot(false):translate(0, M.wheel_slot_right_y, M.wheel_slot_z)

  -- Upper suspension pivot: a cylinder each side joined by a flat bridge.
  local pivot = u.cyl(upper_suspension_height, upper_suspension_radius)
    :rotate(0, 90, 0)

  body = body
    + pivot:translate(
      upper_suspension_x,
      M.upper_suspension_left_y,
      M.upper_suspension_z
    )
    + pivot:translate(
      upper_suspension_x,
      M.upper_suspension_right_y,
      M.upper_suspension_z
    )
    + cube {
      {
        upper_suspension_height,
        spring_support_front_width,
        2 * upper_suspension_radius,
      },
      center = true,
    }:translate(upper_suspension_x, 0, M.upper_suspension_z)

  local support_pin = u.cyl(spring_support_height, spring_support_radius)

  local support = (
    u.rounded_trapezoid(
      spring_support_back_width,
      spring_support_front_width,
      spring_support_length,
      spring_support_height
    )
    + support_pin:translate(
      spring_support_length / 2,
      spring_support_back_width / 2,
      0
    )
    + support_pin:translate(
      spring_support_length / 2,
      -spring_support_back_width / 2,
      0
    )
  )
    :rotate(0, -90, 0)
    :translate(spring_support_x, 0, M.spring_support_z)

  local support_cutout = u.rounded_trapezoid(
    spring_support_cutout_back_width,
    spring_support_cutout_front_width,
    spring_support_cutout_length,
    spring_support_cutout_height
  )
    :rotate(0, -90, 0)
    :translate(spring_support_cutout_x, 0, spring_support_cutout_z)

  return body + (support - support_cutout)
end

--------------------------------------------------------------------------
-- Suspension arm
--
-- A tapered lever with a ball joint standing on its far end.
--------------------------------------------------------------------------

function M.suspension_arm()
  local base_radius = arm_width / 2
  local base_x = base_radius - arm_length / 2

  local end_radius = 8.2 / 2
  local end_x = -end_radius + arm_length / 2

  local link_length = end_x - base_x
  local link_x = end_x - link_length / 2

  return u.cyl(arm_height, base_radius):translate(base_x, 0, 0)
    + u.trapezoid(2 * end_radius, 2 * base_radius, link_length, arm_height)
      :translate(link_x, 0, 0)
    + u.cyl(arm_height, end_radius):translate(end_x, 0, 0)
    + u.ball_head():translate(
      end_x,
      0,
      arm_height / 2 + u.ball_head_height / 2
    )
end

--------------------------------------------------------------------------
-- Front steering rack
--
-- Bolts to the front gearbox and carries the three steering arms.
--------------------------------------------------------------------------

function M.front_steering()
  local trap_length = 15.0
  local trap_front_width = 23.0
  local trap_back_width = 51.0
  local trap_height = 11.22
  local trap_x = box_radius + trap_length / 2
  local trap_z = box_z + trap_height / 2

  local base_length = 11.24
  local base_width = 57.65
  local base_x = trap_x + trap_length / 2 + base_length / 2

  local column_height = (trap_z + trap_height / 2)
    - (M.base_z - M.base_height / 2)
  local column_r = base_length / 2
  local column_left_y = base_width / 2
  local column_right_y = -base_width / 2
  local column_z = trap_z + trap_height / 2 - column_height / 2

  local arm_x = base_x - arm_length / 2
  local arm_z = column_z + column_height / 2 - 20.0

  local arm = M.suspension_arm()
  local column = u.cyl(column_height, column_r)

  return u.rounded_trapezoid(
    trap_front_width,
    trap_back_width,
    trap_length,
    trap_height
  )
    :rotate(0, 0, 180)
    :translate(trap_x, 0, trap_z)
    + cube { { base_length, base_width, trap_height }, center = true }
      :translate(base_x, 0, trap_z)
    + column:translate(base_x, column_left_y, column_z)
    + column:translate(base_x, column_right_y, column_z)
    + arm:rotate(0, 0, 180):translate(arm_x, column_left_y, arm_z)
    + arm:rotate(0, 0, 180):translate(arm_x, column_right_y, arm_z)
    -- The third arm links the two together across the rack.
    + arm:rotate(0, 0, 90):translate(
      arm_x + arm_length / 2,
      column_left_y + arm_length / 2,
      arm_z
    )
end

--------------------------------------------------------------------------
-- Motor cover
--
-- Wraps the back gearbox and holds the drive motor.
--------------------------------------------------------------------------

function M.motor_cover()
  local overlap_length = 16.0
  local overlap_back_width = 43.5
  local overlap_front_width = 34.3
  local overlap_height = 30
  local overlap_x = M.column_front_x + overlap_length / 2
  local overlap_z = M.base_z
    + M.base_height / 2
    + 5.32
    - overlap_height / 2

  local suspension_length = 18.0
  local suspension_back_width = 58.3
  local suspension_front_width = 42.5
  local suspension_height = 5.0
  local suspension_x = overlap_x
    - overlap_length / 2
    + suspension_length / 2
    + 3.5
  local suspension_z = overlap_z
    + overlap_height / 2
    - suspension_height / 2
    - 16.0

  local ball_x = suspension_x - suspension_length / 2 + u.ball_head_length / 2
  local ball_left_y = suspension_back_width / 2 - u.ball_head_width / 2 - 1.5
  local ball_right_y = -suspension_back_width / 2 + u.ball_head_width / 2 + 1.5
  local ball_z = suspension_z + suspension_height / 2 + u.ball_head_height / 2

  local cover_trap_insert_length = 1.0
  local cover_trap_length = 8.0 + cover_trap_insert_length
  local cover_trap_back_width = 2 * 45.5
  local cover_trap_front_width = overlap_front_width + 3.0
  local cover_trap_x = overlap_x
    + overlap_length / 2
    + cover_trap_length / 2
    - cover_trap_insert_length

  local cover_base_length = 13.3
  local cover_base_width = cover_trap_back_width + 2.0
  local cover_base_x = cover_trap_x + cover_trap_length / 2 + cover_base_length / 2

  local cover_cyl_radius = 50.0 / 2
  local cover_cyl_height = 9.6
  local cover_cyl_z = overlap_z + overlap_height / 2 - cover_cyl_radius + 14.5

  local mount_length = 8.0
  local mount_height = 24.0
  local mount_x = cover_base_x + cover_base_length / 2 + mount_length / 2
  local mount_z = overlap_z - overlap_height / 2 + mount_height / 2

  local top_y_scale = cover_base_width / 2
  local top_z_scale = overlap_height - mount_height
  local top_z = mount_z + mount_height / 2

  local motor_radius = 36.0 / 2
  local motor_height = 50.0
  local motor_x = mount_x + mount_length / 2 + motor_height / 2
  local motor_y = -cover_base_width / 4
  local motor_z = mount_z + 5.0

  -- The domed roof over the cover: a cylinder with its lower half sliced off.
  local dome = u.cyl(cover_cyl_height, cover_cyl_radius):rotate(0, 90, 0)
    - cube {
      { cover_cyl_height, 2 * cover_cyl_radius, cover_cyl_radius },
      center = true,
    }:translate(0, 0, -cover_cyl_radius)

  return u.rounded_trapezoid(
    overlap_front_width,
    overlap_back_width,
    overlap_length,
    overlap_height
  ):translate(overlap_x, 0, overlap_z)
    + u.rounded_trapezoid(
      suspension_front_width,
      suspension_back_width,
      suspension_length,
      suspension_height
    ):translate(suspension_x, 0, suspension_z)
    + u.ball_head():translate(ball_x, ball_left_y, ball_z)
    + u.ball_head():translate(ball_x, ball_right_y, ball_z)
    + u.rounded_trapezoid(
      cover_trap_front_width,
      cover_trap_back_width,
      cover_trap_length,
      overlap_height
    )
      :rotate(0, 0, 180)
      :translate(cover_trap_x, 0, overlap_z)
    + cube {
      { cover_base_length, cover_base_width, overlap_height },
      center = true,
    }:translate(cover_base_x, 0, overlap_z)
    + dome:translate(cover_base_x, 0, cover_cyl_z)
    + cube { { mount_length, cover_base_width, mount_height }, center = true }
      :translate(mount_x, 0, mount_z)
    + u.cyl(mount_length, 1.0)
      :rotate(0, 90, 0)
      :scale(1.0, top_y_scale, top_z_scale)
      :translate(mount_x, 0, top_z)
    + u.cyl(motor_height, motor_radius)
      :rotate(0, 90, 0)
      :translate(motor_x, motor_y, motor_z)
end

--------------------------------------------------------------------------
-- Spring plate
--
-- Left off the assembled car -- the original comments it out -- but the
-- part is real and the shock towers key off its dimensions.
--------------------------------------------------------------------------

function M.spring_plate()
  local lower_cutout_length = 8.0
  local lower_cutout_back_width = 15.0
  local lower_cutout_front_width = 10.0
  local lower_cutout_x = plate_upper_cutout_trap_x
    - M.plate_upper_cutout_trap_length / 2
    - lower_cutout_length / 2
    - 3.5

  local plate = cube {
    { M.plate_base_length, spring_support_back_width, M.plate_base_height },
    center = true,
  }
    + u.trapezoid(
      plate_lower_trap_front_width,
      plate_lower_trap_back_width,
      plate_lower_trap_length,
      M.plate_base_height
    )
      :rotate(0, 180, 0)
      :translate(plate_lower_trap_x, 0, 0)
    + u.trapezoid(
      M.plate_upper_trap_front_width,
      M.plate_upper_trap_back_width,
      M.plate_upper_trap_length,
      M.plate_upper_trap_height
    ):translate(M.plate_upper_trap_x, 0, 0)

  plate = plate
    - u.rounded_trapezoid(
      M.plate_upper_cutout_trap_front_width,
      M.plate_upper_cutout_trap_back_width,
      M.plate_upper_cutout_trap_length,
      M.plate_upper_trap_height
    )
      :scale(1.0, 1.0, 1.0001)
      :rotate(0, 180, 0)
      :translate(plate_upper_cutout_trap_x, 0, 0)
    - u.rounded_trapezoid(
      lower_cutout_front_width,
      lower_cutout_back_width,
      lower_cutout_length,
      M.plate_upper_trap_height
    )
      :scale(1.0, 1.0, 1.0001)
      :rotate(0, 180, 0)
      :translate(lower_cutout_x, 0, 0)

  -- Rounds every edge by 2 mm.
  return plate
    :minkowski(u.cyl(0.001, 2.0))
    :translate(M.plate_base_length / 2, 0, 0)
    :rotate(0, -90, 0)
    :translate(
      spring_support_x - spring_support_height / 2 + M.plate_base_height / 2,
      0,
      M.spring_support_z + spring_support_length / 2
    )
end

return M
