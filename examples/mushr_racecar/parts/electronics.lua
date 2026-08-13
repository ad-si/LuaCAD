-- The payload the car carries: steering servo, LiPo packs, Jetson Nano,
-- the two RealSense cameras and the YDLidar.
--
-- Ported from v3/scad/racecar_servo_motor.scad, racecar_battery.scad,
-- racecar_jetson_nano.scad, racecar_d435.scad, racecar_t265.scad and
-- racecar_ydlidar.scad.
--
-- These are stand-in models of bought-in parts, not printable geometry --
-- accurate enough to check fit and to make a render read as a real robot.

local u = require("parts.utils")
local suspension = require("parts.suspension")

local M = {}

--------------------------------------------------------------------------
-- Steering servo
--------------------------------------------------------------------------

M.servo_body_length = 40.12
M.servo_body_width = 40.5
M.servo_body_height = 20.2

local faceplate_radius = 3.38
local faceplate_length = 54.5 - 2 * faceplate_radius
local faceplate_width = 3.2
local faceplate_height = 18.8 - 2 * faceplate_radius
M.servo_faceplate_y = -M.servo_body_width / 2 + faceplate_width / 2 + 9.85
M.servo_faceplate_width = faceplate_width

local arm_length = 14.8
local arm_width = 5.95

local gear_radius = arm_length / 2
local gear_x = -M.servo_body_length / 2 + 8.5
local gear_y = -M.servo_body_width / 2 - arm_width / 2

local shaft_length = 8.0
local shaft_height = 14.87
local shaft_z = 24.0 - shaft_height / 2

local tip_radius = shaft_length / 2
local tip_z = shaft_z + shaft_height / 2

local screw_hole_radius = 3.25 / 2
local screw_lower_z = tip_z - 4.0

local ball_y = gear_y - arm_width / 2 - u.ball_head_height / 2

function M.servo_body()
  local hole_radius = 5 / 2
  local hole_front_x = 50 / 2
  local hole_upper_z = 10.0 / 2

  -- The mounting flange: a rounded rectangle standing on edge.
  local corner = u.cyl(faceplate_width, faceplate_radius):rotate(90, 0, 0)
  local plate = (
    cube {
      { faceplate_length, faceplate_width, faceplate_height },
      center = true,
    }
    + corner:translate(faceplate_length / 2, 0, faceplate_height / 2)
    + corner:translate(faceplate_length / 2, 0, -faceplate_height / 2)
    + corner:translate(-faceplate_length / 2, 0, faceplate_height / 2)
    + corner:translate(-faceplate_length / 2, 0, -faceplate_height / 2)
  ):hull()

  local hole = u.cyl(faceplate_width, hole_radius):rotate(90, 0, 0)

  for _, x in ipairs { hole_front_x, -hole_front_x } do
    for _, z in ipairs { hole_upper_z, -hole_upper_z } do
      plate = plate - hole:translate(x, 0, z)
    end
  end

  return cube {
    { M.servo_body_length, M.servo_body_width, M.servo_body_height },
    center = true,
  } + plate:translate(0, M.servo_faceplate_y, 0)
end

function M.servo_arm()
  local hub = (
    u.cyl(arm_width, gear_radius)
      :rotate(90, 0, 0)
      :translate(gear_x, gear_y, 0)
    + cube { { shaft_length, arm_width, 1.0 }, center = true }
      :translate(gear_x, gear_y, shaft_z - shaft_height / 2 - 0.5)
  ):hull()

  local hole = u.cyl(arm_width, screw_hole_radius):rotate(90, 0, 0)

  local arm = cube { { shaft_length, arm_width, shaft_height }, center = true }
    :translate(gear_x, gear_y, shaft_z)
    + u.cyl(arm_width, tip_radius)
      :rotate(90, 0, 0)
      :translate(gear_x, gear_y, tip_z)

  return hub
    + (arm - hole:translate(gear_x, gear_y, tip_z) - hole:translate(
      gear_x,
      gear_y,
      screw_lower_z
    ))
    + u.ball_head():rotate(90, 0, 0):translate(gear_x, ball_y, screw_lower_z)
end

-- The drag link from the servo arm across to the steering rack.
function M.servo_arm_link()
  local width = 67.75

  return suspension
    .link(width, 90)
    :rotate(0, 0, 90)
    :rotate(0, 0, 8)
    :rotate(0, 12, 0)
    :translate(
      gear_x + width / 2 - u.ball_head_length / 2 - 2,
      ball_y + 3,
      screw_lower_z - 6
    )
end

--------------------------------------------------------------------------
-- Battery
--------------------------------------------------------------------------

M.battery_length = 135.0
M.battery_width = 46.8
M.battery_height = 24.0

-- The pack is a rounded bar; the middle section is slightly narrower than
-- the two moulded ends.
local function pack_section(width, length)
  return u.rounded_square(width, M.battery_height, length)
    :rotate(0, 0, 90)
    :rotate(0, 90, 0)
end

function M.battery()
  local end_length = 10.75
  local end_x = M.battery_length / 2 - end_length / 2
  local body_length = M.battery_length - 2 * end_length

  return pack_section(M.battery_width, end_length):translate(end_x, 0, 0)
    + pack_section(M.battery_width, end_length):translate(-end_x, 0, 0)
    + pack_section(42.75, body_length)
end

--------------------------------------------------------------------------
-- Jetson Nano
--------------------------------------------------------------------------

M.jetson_length = 80.0
M.jetson_width = 100.0
M.jetson_height = 1.75
M.jetson_support_height = 4.2

function M.jetson_nano()
  local half_l = M.jetson_length / 2
  local half_w = M.jetson_width / 2
  local top = M.jetson_height / 2

  -- Everything on the board is a box sitting on the top face, given as
  -- {length, width, height, x, y} with x, y measured from the near edges.
  local parts = {
    { 49.0, 69.68, 26.75, half_l - 49.0 / 2, 0 }, -- heatsink
    { 14.22, 9.1, 11.0, -half_l + 14.22 / 2 - 1.5, half_w - 9.1 / 2 - 1.5 },
    { 18.15, 17.0, 18.41, -half_l + 18.15 / 2 - 1.5, half_w - 17.0 / 2 - 16.5 },
    { 17.6, 13.2, 15.4, -half_l + 17.6 / 2 - 1.5, half_w - 13.2 / 2 - 37.0 },
    { 17.6, 13.2, 15.4, -half_l + 17.6 / 2 - 1.5, half_w - 13.2 / 2 - 53.75 },
    { 21.5, 16.0, 13.67, -half_l + 21.5 / 2 - 1.5, -half_w + 16.0 / 2 + 14.75 },
    { 6.75, 8.13, 2.75, -half_l + 6.75 / 2 - 0.9, -half_w + 8.13 / 2 + 4.5 },
  }

  local board = cube {
    { M.jetson_length, M.jetson_width, M.jetson_height },
    center = true,
  }

  for _, p in ipairs(parts) do
    board = board
      + cube { { p[1], p[2], p[3] }, center = true }
        :translate(p[4], p[5], top + p[3] / 2)
  end

  -- Standoffs underneath
  local support = u.cyl(M.jetson_support_height, 5 / 2)
  local support_front_x = half_l - 3.5
  local support_left_y = half_w - 3.0
  local support_z = -top - M.jetson_support_height / 2

  for _, x in ipairs { support_front_x, support_front_x - 58.25 } do
    board = board
      + support:translate(x, support_left_y, support_z)
      + support:translate(x, support_left_y - 75.0, support_z)
  end

  return board
end

--------------------------------------------------------------------------
-- RealSense D435 depth camera
--------------------------------------------------------------------------

M.d435_length = 25.15

local function d435_face(width, height, thickness, x)
  return u.rounded_square(width, height, thickness)
    :rotate(0, 0, 90)
    :rotate(0, 90, 0)
    :translate(x, 0, 0)
end

local function d435_front()
  return d435_face(84.0, 19.5, 1.0, M.d435_length / 2 - 1.0 / 2)
end

function M.d435()
  local hole_length = 8.0
  local hole_width = 1.75
  local hole_height = M.d435_length / 2
  local hole_spacing = 4.5 - hole_width
  local hole_x = -M.d435_length / 2 + hole_length / 2 + 3.0
  local hole_z = 25.0 / 2 - hole_height / 2

  local shell = (
    d435_front() + d435_face(90.0, 25.0, 1.0, -M.d435_length / 2 + 1.0 / 2)
  ):hull()

  -- The vent grille across the top face
  local vent = u.rounded_square(hole_length, hole_width, hole_height)
  for i = 0, 10 do
    shell = shell
      - vent:translate(hole_x, i * hole_spacing, hole_z)
      - vent:translate(hole_x, -i * hole_spacing, hole_z)
  end

  -- Hollow it out, then drop the lens panel back in.
  return shell - d435_front():scale(1.0, 0.98, 0.9)
end

-- The dark lens panel that sits in the shell's mouth. The original draws
-- this inside the camera module -- it is the one place in the whole model
-- that calls color() -- so it is kept as its own part here.
function M.d435_screen()
  return d435_front():scale(0.99, 0.98, 0.9)
end

--------------------------------------------------------------------------
-- RealSense T265 tracking camera
--------------------------------------------------------------------------

M.t265_length = 12.5
local t265_width = 108
local t265_height = 24.43

-- The camera's face: a rounded slab standing on edge.
local function t265_front(length)
  local radius = 4.5
  local corner = u.cyl(length, radius):rotate(0, 90, 0)
  local y = t265_width / 2 - radius
  local z = t265_height / 2 - radius

  return (
    corner:translate(0, y, z)
    + corner:translate(0, y, -z)
    + corner:translate(0, -y, z)
    + corner:translate(0, -y, -z)
  ):hull()
end

function M.t265()
  local front_length = 8.2

  -- Both the lens panel and the tapered back are the face profile scaled
  -- down; scaling Y and Z together keeps the corner radius even.
  local y_scale = 0.99
  local z_scale = 1.0 - (1 - y_scale) * (t265_width / t265_height)
  local shift = t265_width * (1.0 - y_scale) / 2

  local screen = t265_front(0.25)
    :scale(1.0, y_scale, z_scale)
    :translate(M.t265_length / 2 - 0.25 / 2 + 0.01, 0, 0)

  local face = t265_front(front_length)
    :translate(M.t265_length / 2 - front_length / 2, 0, 0) - screen

  local back_radius = 3
  local back_x = -M.t265_length / 2 + back_radius / 2
  local back_y = t265_width / 2 - shift - back_radius / 2
  local back_z = t265_height / 2 - shift - back_radius / 2

  local quarter = u.quarter_sphere(back_radius)

  local body = (
    t265_front(0.1)
      :scale(1.0, y_scale, z_scale)
      :translate(M.t265_length / 2 - front_length, 0, 0)
    + quarter:rotate(0, 0, 90):translate(back_x, back_y, back_z)
    + quarter:rotate(0, 90, 90):translate(back_x, back_y, -back_z)
    + quarter:rotate(0, 0, 180):translate(back_x, -back_y, back_z)
    + quarter:rotate(90, 180, 0):translate(back_x, -back_y, -back_z)
  ):hull()

  return face + body
end

--------------------------------------------------------------------------
-- YDLidar X4
--------------------------------------------------------------------------

M.lidar_length = 101.7
M.lidar_height = 59.0

-- The lidar's own model uses a coarser facet count than the rest of the car.
local lidar_fn = 50

local function lidar_cyl(h, r)
  return cylinder { h = h, r = r, center = true, fn = lidar_fn }
end

local scanner_radius = 65.97 / 2
local scanner_height = 25.03
local scanner_z = M.lidar_height / 2 - scanner_height / 2

local rim_height = 3.5
local rim_radius = 68.63 / 2
local rim_z = scanner_z - scanner_height / 2 + rim_height / 2

local base_front_radius = 71.0 / 2
local base_back_radius = 35 / 2
local base_back_x = base_front_radius - M.lidar_length + base_back_radius

local leg_radius = 9.5 / 2
M.lidar_leg_front_x = 22.0
local lidar_leg_back_x = -35.0
M.lidar_leg_front_left_y = 31.0
local lidar_leg_back_left_y = 25.0

local base_height = 10.0
local foot_height = 4.5
local lower_platform_height = 2.0

-- The rotating scanner head, with its emitter and receiver windows.
function M.lidar_top()
  local emitter_height = 13.0
  local emitter_radius = 10.5 / 2
  local emitter_x = scanner_radius - emitter_height / 2 - 2
  local emitter_y = -rim_radius / 2
  local emitter_z = scanner_z + 3

  return lidar_cyl(scanner_height, scanner_radius):translate(0, 0, scanner_z)
    + lidar_cyl(rim_height, rim_radius):translate(0, 0, rim_z)
    - lidar_cyl(emitter_height, emitter_radius)
      :rotate(0, 90, 0)
      :translate(emitter_x, emitter_y, emitter_z)
    - u.rounded_square(15.0, 2 * emitter_radius, emitter_height)
      :rotate(0, 0, 90)
      :rotate(0, 90, 0)
      :translate(emitter_x + 1.0, -emitter_y, emitter_z)
end

-- One deck of the mount: a teardrop platform with four legs blended in.
local function lidar_base(height, leg_thickness)
  local deck_z = scanner_z - scanner_height / 2 - height / 2

  local deck = (
    lidar_cyl(height, base_front_radius):translate(0, 0, deck_z)
    + lidar_cyl(height, base_back_radius):translate(base_back_x, 0, deck_z)
  ):hull()

  -- Each foot is hulled to a hidden disc at the centre, so the legs fair
  -- smoothly into the deck rather than sticking out as bare cylinders.
  local hidden = lidar_cyl(leg_thickness, 20.0):translate(0, 0, deck_z)
  local feet = {
    { M.lidar_leg_front_x, M.lidar_leg_front_left_y },
    { lidar_leg_back_x, lidar_leg_back_left_y },
    { M.lidar_leg_front_x, -M.lidar_leg_front_left_y },
    { lidar_leg_back_x, -lidar_leg_back_left_y },
  }

  for _, f in ipairs(feet) do
    deck = deck
      + (
        hidden
        + lidar_cyl(leg_thickness, leg_radius):translate(f[1], f[2], deck_z)
      ):hull()
  end

  return deck
end

function M.lidar_mount()
  local leg_height = M.lidar_height - scanner_height - base_height / 2
  local leg_z = scanner_z
    - scanner_height / 2
    - base_height / 2
    - leg_height / 2

  local post = u.hexagon(leg_height, 5.0)

  return lidar_base(base_height, foot_height)
    + lidar_base(lower_platform_height, lower_platform_height)
      :translate(0, 0, -17.5)
    + post:translate(M.lidar_leg_front_x, M.lidar_leg_front_left_y, leg_z)
    + post:translate(lidar_leg_back_x, lidar_leg_back_left_y, leg_z)
    + post:translate(M.lidar_leg_front_x, -M.lidar_leg_front_left_y, leg_z)
    + post:translate(lidar_leg_back_x, -lidar_leg_back_left_y, leg_z)
end

function M.lidar_motor()
  local height = 20.0
  local z = scanner_z - scanner_height / 2 - base_height - height / 2

  return lidar_cyl(height, 32.0 / 2):translate(base_back_x, 0, z)
end

return M
