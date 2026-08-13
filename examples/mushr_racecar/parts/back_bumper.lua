-- Rear bumper: the wide bar that cantilevers off the back gearbox.
--
-- Ported from chassis/racecar_chassis_back_bumper.scad.
--
-- Coordinates are relative to the back gearbox, which is where the
-- assembly hangs it.

local u = require("parts.utils")
local gearbox = require("parts.gearbox")

local M = {}

local attach_length = 10.21
local attach_width = 6.00
local attach_height = 6.0
local attach_x = -gearbox.base_length / 2 - attach_length / 2
local attach_left_y = gearbox.base_width / 2 - attach_width / 2
local attach_right_y = -gearbox.base_width / 2 + attach_width / 2
local attach_z = gearbox.base_z + gearbox.base_height / 2 - attach_height / 2

local extend_length = 10.16
local extend_back_width = attach_width + (attach_left_y - attach_right_y) - 2.0
local extend_front_width = 20.0
local extend_height = 6.0
local extend_x = attach_x - attach_length / 2 + extend_height / 2
local extend_z = attach_z + attach_height / 2 + extend_length / 2

local mount_cyl_height = 9.23
local mount_cyl_radius = extend_height / 2
local mount_trap_length = 50.0
local mount_trap_front_width = 40.0
local mount_trap_back_width = 78.0
local mount_trap_height = 5.0
local mount_pitch = -20.0
local mount_x = extend_x - 1.5
local mount_z = extend_z + extend_length / 2 + mount_cyl_radius / 2 + 1

local bar_base = 8.8
local bar_height = 5.0
local bar_length = 268
local bar_x = mount_x
  - extend_length / 2
  - mount_trap_length * cos(abs(mount_pitch))
  + 3
local bar_z = mount_z
  + mount_cyl_height / 2
  - mount_trap_length * sin(abs(mount_pitch))
  + 3

local support_height = extend_x - bar_x
local support_x = 0.5 * (extend_x + bar_x) + 2

local top_length = 25.0
local top_front_width = 100.0
local top_back_width = 221.0
local top_z = bar_z + top_length / 2 + 1.0

local flap_length = 1.5
local flap_width = 40.0
local flap_height = 50.5
local flap_radius = 3.3
local flap_x = bar_x + bar_height / 2 + flap_length / 2
local flap_left_y = bar_length / 2 - flap_width / 2
local flap_z = bar_z + bar_base / 2 - flap_height / 2

--------------------------------------------------------------------------

-- The bracket that carries the bar, tilted 20 degrees down at the back.
local function bar_mount()
  local pin = u.cyl(mount_cyl_height, mount_cyl_radius)
  local plate_y = extend_front_width / 2 - mount_cyl_radius - 2.45

  -- A tapered plate with the middle removed, leaving a flat hoop.
  local plate = u.rounded_trapezoid(
    mount_trap_front_width,
    mount_trap_back_width,
    mount_trap_length,
    mount_trap_height
  ) - u.rounded_trapezoid(
    mount_trap_front_width - 10,
    mount_trap_back_width - 10,
    mount_trap_length - 12,
    mount_trap_height
  )

  return (
    pin:translate(0, plate_y, 0)
    + pin:translate(0, -plate_y, 0)
    + plate:translate(
      -mount_trap_length / 2 + mount_cyl_radius - 1.0,
      0,
      mount_cyl_height / 2 + mount_trap_height / 2
    )
  ):rotate(0, mount_pitch, 0)
end

-- The upswept spoiler above the bar: a tapered plate with a slot milled
-- out and both tapered flanks pared back.
local function top_plate()
  local slot_length = top_length - 1.5
  local flank_depth = (top_back_width - top_front_width) / 2
  local flank = u.trap_triangle(mount_trap_height + 0.01, slot_length, flank_depth)

  return u.rounded_trapezoid(
    top_front_width,
    top_back_width,
    top_length,
    bar_height
  )
    - cube { { slot_length, top_front_width - 10.0, bar_height }, center = true }
      :translate(-(top_length + 2) / 2 + slot_length / 2, 0, 0)
    - flank:rotate(-90, 0, 0):translate(
      -(top_length + 2) / 2,
      top_front_width / 2,
      bar_height / 2
    )
    - flank:rotate(90, 0, 0):translate(
      -(top_length + 2) / 2,
      -top_front_width / 2,
      -bar_height / 2 - 0.001
    )
end

-- A thin rounded fin at each end of the bar.
local function flap()
  local inner_w = flap_width - 2 * flap_radius
  local inner_h = flap_height - 2 * flap_radius
  local pin = u.cyl(flap_length, flap_radius):rotate(0, 90, 0)

  return (
    cube { { flap_length, inner_w, inner_h }, center = true }
    + pin:translate(0, inner_w / 2, inner_h / 2)
    + pin:translate(0, -inner_w / 2, inner_h / 2)
    + pin:translate(0, inner_w / 2, -inner_h / 2)
    + pin:translate(0, -inner_w / 2, -inner_h / 2)
  ):hull()
end

function M.back_bumper()
  local tab = cube {
    { attach_length, attach_width, attach_height },
    center = true,
  }
  local support = u.cyl(support_height, mount_cyl_radius):rotate(0, 90, 0)

  return tab:translate(attach_x, attach_left_y, attach_z)
    + tab:translate(attach_x, attach_right_y, attach_z)
    + u.rounded_trapezoid(
      extend_front_width,
      extend_back_width,
      extend_length,
      extend_height
    )
      :rotate(0, -90, 0)
      :translate(extend_x, 0, extend_z)
    + bar_mount():translate(mount_x, 0, mount_z)
    + u.bh_triangle(bar_base, bar_height, bar_length)
      :rotate(0, -90, 0)
      :translate(bar_x, 0, bar_z)
    + support:translate(support_x, 10.0, bar_z)
    + support:translate(support_x, -10.0, bar_z)
    + top_plate():rotate(0, -90, 0):translate(bar_x, 0, top_z)
    + flap():translate(flap_x, flap_left_y, flap_z)
    + flap():translate(flap_x, -flap_left_y, flap_z)
end

return M
