-- Suspension linkage: wheel shaft, upper and lower control arms, the
-- adjustable turnbuckle link, and the front bumper absorber.
--
-- Ported from chassis/racecar_chassis_wheel_shaft.scad,
-- racecar_chassis_lower_suspension_link.scad,
-- racecar_chassis_upper_suspension_link.scad,
-- racecar_chassis_link.scad and racecar_chassis_bumper_absorber.scad.
--
-- Every link runs along Y, so its two ends sit at +/- width/2.

local u = require("parts.utils")

local M = {}

-- Head dimensions other modules position against.
M.link_head_length = 8.0
M.link_head_width = 21.0
M.link_head_height = 5.6

local upper_head_length = 20.82
local upper_head_width = 26.8
local upper_head_height = 7.0

M.upper_head_width = upper_head_width

--------------------------------------------------------------------------
-- Wheel shaft
--
-- A thin rod with a ball at each end, running through the hub.
--------------------------------------------------------------------------

function M.wheel_shaft(width)
  width = width or 73.35

  local length = 10.0
  local height = 6.75

  local head_radius = height / 2
  local head_left_y = width / 2 - head_radius
  local head_right_y = -width / 2 + head_radius

  local body_radius = 3.5 / 2
  local body_height = width - head_radius

  local ball = sphere { r = head_radius, fn = 25 }
  local stub = cylinder {
    h = length,
    r = 3 / 2,
    center = true,
    fn = u.fn,
  }:rotate(0, 90, 0)

  return cylinder {
    h = body_height,
    r = body_radius,
    center = true,
    fn = u.fn,
  }:rotate(90, 0, 0)
    + ball:translate(0, head_left_y, 0)
    + stub:translate(0, head_left_y, 0)
    + ball:translate(0, head_right_y, 0)
    + stub:translate(0, head_right_y, 0)
end

--------------------------------------------------------------------------
-- Lower control arm
--
-- A wide A-arm: a tapered slab with a triangular window cut out of it,
-- pivoting on a fat cylinder at each end.
--------------------------------------------------------------------------

function M.lower_link(width)
  width = width or 65.67

  local length = 31.86
  local height = 10.0

  local platform_end_radius = height / 2
  local platform_end_y = -width / 2 + platform_end_radius

  local wheel_end_radius = 3.5
  local wheel_end_y = width / 2 - wheel_end_radius

  local insert_depth = 8.0
  local body_length = width
    - 2 * platform_end_radius
    - 2 * wheel_end_radius
    + insert_depth
  local body_back_width = length
  local body_front_width = 24.0
  local body_height = 6.75
  local body_y = 0.5 * (platform_end_y + wheel_end_y)

  -- The window is a rectangle with its two long corners sliced off, so
  -- the remaining material follows the taper of the arm.
  local margin = 5
  local cutout_length = body_length - margin - insert_depth
  local cutout_width = body_front_width - margin
  local cutout_angle = atan(cutout_width / cutout_length)

  local wedge = cube {
    { 2 * cutout_length, margin, body_height },
    center = true,
  }
  local window = cube { { cutout_length, cutout_width, body_height }, center = true }
    - wedge:rotate(0, 0, cutout_angle)
    - wedge:rotate(0, 0, -cutout_angle)

  local body = (
    u.trapezoid(body_front_width, body_back_width, body_length, body_height)
    - window
  )
    :rotate(0, 0, 90)
    :translate(0, body_y, 0)

  return cylinder {
    h = length,
    r = platform_end_radius,
    center = true,
    fn = u.fn,
  }
      :rotate(0, 90, 0)
      :translate(0, platform_end_y, 0)
    + cylinder { h = length, r = wheel_end_radius, center = true, fn = u.fn }
      :rotate(0, 90, 0)
      :translate(0, wheel_end_y, 0)
    + body
end

--------------------------------------------------------------------------
-- Upper control arm
--------------------------------------------------------------------------

local function upper_link_head()
  local base_length = 9.8
  local base_back_width = 11.0
  local base_front_width = 8.75
  local base_height = upper_head_height
  local base_y = base_length / 2 - upper_head_width / 2

  local trap_length = 4.0
  local trap_y = base_y + base_length / 2 + trap_length / 2

  local end_radius = upper_head_height / 2
  local end_y = upper_head_width / 2 - end_radius

  local cube_width = upper_head_width
    - base_length
    - trap_length
    - end_radius
  local cube_y = trap_y + trap_length / 2 + cube_width / 2

  return u.trapezoid(base_front_width, base_back_width, base_length, base_height)
      :rotate(0, 0, -90)
      :translate(0, base_y, 0)
    + u.trapezoid(base_back_width, upper_head_length, trap_length, base_height)
      :rotate(0, 0, -90)
      :translate(0, trap_y, 0)
    + cylinder {
      h = upper_head_length,
      r = end_radius,
      center = true,
      fn = u.fn,
    }
      :rotate(0, 90, 0)
      :translate(0, end_y, 0)
    + cube {
      { upper_head_length, cube_width, base_height },
      center = true,
    }:translate(0, cube_y, 0)
end

function M.upper_link(width)
  width = width or 71.33

  local rod_height = width - 2 * upper_head_width
  local head = upper_link_head()

  return (
    cylinder { h = rod_height, r = 4.0 / 2, center = true, fn = u.fn }
    + u.hexagon(6.0, 4.5)
  ):rotate(90, 0, 0)
    + head:translate(0, width / 2 - upper_head_width / 2, 0)
    + head:rotate(0, 0, 180):translate(0, -width / 2 + upper_head_width / 2, 0)
end

--------------------------------------------------------------------------
-- Steering / camber link
--
-- A threaded rod with a hex adjuster, ending in a pierced eye at each end.
--------------------------------------------------------------------------

local function link_head()
  local head_outer_radius = M.link_head_length / 2
  local head_inner_radius = 5.25 / 2
  local head_height = 3.75
  local head_y = -head_outer_radius + M.link_head_width / 2

  local base_diameter = M.link_head_height
  local base_height = 9.5
  local base_y = base_height / 2 - M.link_head_width / 2

  local connect_insert_depth = 1.0
  local connect_length = M.link_head_width
    - base_height
    - 2 * head_outer_radius
    + connect_insert_depth
  local connect_height = base_diameter * tan(30)
  local connect_y = base_y + base_height / 2 + connect_length / 2

  -- Hull from the hex adjuster out to the tapered neck.
  local shank = (
    u.hexagon(base_height, base_diameter)
      :rotate(90, 30, 0)
      :translate(0, base_y, 0)
    + u.trapezoid(head_height, base_diameter, connect_length, connect_height)
      :rotate(0, 0, 90)
      :translate(0, connect_y, 0)
  ):hull()

  local eye = cylinder {
    h = head_height,
    r = head_outer_radius,
    center = true,
    fn = u.fn,
  } - cylinder {
    h = head_height,
    r = head_inner_radius,
    center = true,
    fn = u.fn,
  }

  return shank + eye:translate(0, head_y, 0)
end

function M.link(width, head_pitch)
  width = width or 58.72
  head_pitch = head_pitch or 0.0

  local rod_height = width - 2 * M.link_head_width
  local head = link_head()

  return (
    cylinder { h = rod_height, r = 3.0 / 2, center = true, fn = u.fn }
    + u.hexagon(7.0, 4.0)
  ):rotate(90, 0, 0)
    + head
      :rotate(0, head_pitch, 0)
      :translate(0, width / 2 - M.link_head_width / 2, 0)
    + head
      :rotate(0, 0, 180)
      :translate(0, -width / 2 + M.link_head_width / 2, 0)
end

--------------------------------------------------------------------------
-- Front bumper absorber
--------------------------------------------------------------------------

M.absorber_length = 43.15
M.absorber_width = 59.15
M.absorber_height = 6.0

-- `column_span` is the gearbox column spacing the two tabs bolt to.
function M.bumper_absorber(column_span)
  local box_attach_length = 18.15
  local box_attach_width = 7.5
  local box_attach_x = box_attach_length / 2 - M.absorber_length / 2

  local base_length = 17.6
  local base_x = box_attach_x + box_attach_length / 2 + base_length / 2

  local attach_radius = M.absorber_height / 2
  local attach_height = M.absorber_length - box_attach_length - base_length
  local attach_x = base_x + base_length / 2 + attach_height / 2

  local tab = cube {
    { box_attach_length, box_attach_width, M.absorber_height },
    center = true,
  }

  -- A rounded rectangle with a smaller copy of itself removed, leaving a hoop.
  local hoop = (
    u.rounded_square(M.absorber_width, base_length, M.absorber_height)
    - u.rounded_square(M.absorber_width, base_length, M.absorber_height)
      :scale(0.93, 0.8, 1.0)
  )
    :rotate(0, 0, 90)
    :translate(base_x, 0, 0)

  local pin = cylinder {
    h = attach_height,
    r = attach_radius,
    center = true,
    fn = u.fn,
  }:rotate(0, 90, 0)

  return tab:translate(box_attach_x, column_span / 2, 0)
    + tab:translate(box_attach_x, -column_span / 2, 0)
    + hoop
    + pin:translate(attach_x, 30.0 / 2, 0)
    + pin:translate(attach_x, -30.0 / 2, 0)
end

return M
