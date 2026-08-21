-- The rolling chassis: platform, both gearboxes, all four corners of
-- suspension, the wheels and the bumpers.
--
-- Ported from chassis/racecar_chassis.scad.
--
-- Returns a list of separately coloured parts rather than one welded
-- solid, so the export keeps them as distinct objects.

local u = require("parts.utils")
local palette = require("parts.palette")
local platform = require("parts.platform")
local gearbox = require("parts.gearbox")
local wheel = require("parts.wheel")
local suspension = require("parts.suspension")
local shock = require("parts.shock_tower")
local back_bumper = require("parts.back_bumper")

local M = {}

--------------------------------------------------------------------------
-- Where everything sits
--------------------------------------------------------------------------

M.back_gearbox_x = platform.back_x - 5.0
M.front_gearbox_x = platform.front_x - 17.0
M.gearbox_z = platform.height / 2

M.wheel_left_y = 115.0
M.wheel_right_y = -115.0
M.wheel_z = M.gearbox_z + gearbox.base_height / 2

local absorber_x = M.front_gearbox_x
  + gearbox.base_length / 2
  + suspension.absorber_length / 2
  - 3.0
local absorber_z = M.gearbox_z
  + gearbox.base_height
  + suspension.absorber_height / 2

local upper_link_width = 71.33
local upper_link_left_y = gearbox.upper_suspension_left_y + upper_link_width / 2
local upper_link_right_y = gearbox.upper_suspension_right_y
  - upper_link_width / 2
local upper_link_z = M.gearbox_z + gearbox.upper_suspension_z

local wheel_shaft_width = 58.35
local wheel_shaft_left_y = gearbox.wheel_slot_left_y
  + wheel_shaft_width / 2
  + 3.0
local wheel_shaft_z = M.gearbox_z + gearbox.wheel_slot_z - 3

local lower_link_width = 68.67
local lower_link_left_y = platform.back_suspension_connector_left_y
  + lower_link_width / 2
  - platform.suspension_connector_radius
local lower_link_z = platform.back_suspension_connector_z - 2

local link_width = 67.72
local link_front_x = M.front_gearbox_x
  - wheel.outer_suspension_link_back_width / 2
  + u.ball_head_length / 2
  + 2.25
local link_back_x = M.back_gearbox_x
  + wheel.outer_suspension_link_back_width / 2
  - u.ball_head_length / 2
  - 2.25
local link_left_y = M.wheel_left_y
  - wheel.suspension_link_y_offset
  - wheel.outer_suspension_link_length / 2
  + u.ball_head_width / 2
  + 1
  - link_width / 2
  + suspension.link_head_length / 2
  - 1.0
local link_z = M.wheel_z
  + wheel.outer_suspension_link_height / 2
  + u.ball_head_height
  - suspension.link_head_height / 2

local shock_height = 102.3
local shock_front_x = M.front_gearbox_x
  + gearbox.plate_upper_trap_x
  + gearbox.plate_upper_trap_height / 2
  + shock.connector_length / 2
  - 6
local shock_back_x = M.back_gearbox_x
  - gearbox.plate_upper_trap_x
  - gearbox.plate_upper_trap_height / 2
  - shock.connector_length / 2
  + 6
local shock_left_y = 0.5
  * (
    gearbox.plate_upper_trap_front_width / 2
    + gearbox.plate_upper_cutout_trap_back_width / 2
  )
  - 3
-- The original folds an X offset into this Z sum. It is almost certainly a
-- slip, but it is what positions the towers in every published render, so
-- it is reproduced rather than corrected.
local shock_z = M.gearbox_z
  + gearbox.plate_upper_trap_x
  + gearbox.spring_support_z
  + gearbox.spring_support_length / 2
  + gearbox.plate_base_length / 2
  + gearbox.plate_upper_trap_length / 2
  + 0.5

--------------------------------------------------------------------------
-- Assembly
--------------------------------------------------------------------------

-- Each entry describes one exported object. Keeping the colour, material
-- and name alongside the solid -- rather than baking them in -- lets
-- callers filter the list, which is what the OpenSCAD comparison harness
-- does.
function M.parts()
  local out = {}
  -- `look` names a role in the palette, which supplies the colour and,
  -- for some roles, a surface material.
  local function add(solid, look, name)
    out[#out + 1] = {
      solid = solid,
      color = palette.colors[look],
      material = palette.materials[look],
      name = name,
    }
  end

  add(platform.platform(), "deck", "platform")

  -- Gearboxes. The front one is the same moulding turned around, with the
  -- steering rack bolted on.
  add(
    gearbox.gearbox():translate(M.back_gearbox_x, 0, M.gearbox_z),
    "gearbox",
    "gearbox-rear"
  )
  add(
    gearbox.motor_cover():translate(M.back_gearbox_x, 0, M.gearbox_z),
    "cover",
    "motor-cover"
  )
  add(
    (gearbox.gearbox() + gearbox.front_steering())
      :rotate(0, 0, 180)
      :translate(M.front_gearbox_x, 0, M.gearbox_z),
    "gearbox",
    "gearbox-front"
  )

  add(
    back_bumper.back_bumper():translate(M.back_gearbox_x, 0, M.gearbox_z),
    "bumper",
    "bumper-rear"
  )
  add(
    suspension
      .bumper_absorber(gearbox.base_width)
      :scale(1.2, 1.0, 1.0)
      :translate(absorber_x, 0, absorber_z),
    "bumper",
    "bumper-absorber"
  )

  -- Wheels. Tyre and hub are separate objects so the rubber and the
  -- printed centre can be different colours.
  -- `ball` picks which side of the upright the steering ball sits on. The
  -- pattern is diagonal, not per-side: the tie rods all run inboard and
  -- forward, so the front and rear uprights on one side are handed
  -- opposite ways.
  local corners = {
    { x = M.front_gearbox_x, y = M.wheel_right_y, flip = false, ball = true },
    { x = M.front_gearbox_x, y = M.wheel_left_y, flip = true, ball = false },
    { x = M.back_gearbox_x, y = M.wheel_left_y, flip = true, ball = true },
    { x = M.back_gearbox_x, y = M.wheel_right_y, flip = false, ball = false },
  }

  local tire = wheel.tire()
  local hub = wheel.hub()

  for i, c in ipairs(corners) do
    local yaw = c.flip and 180 or 0
    local axle = wheel.axle(c.ball)

    add(
      tire:rotate(0, 0, yaw):translate(c.x, c.y, M.wheel_z),
      "tire",
      "tire-" .. i
    )
    add(
      hub:rotate(0, 0, yaw):translate(c.x, c.y, M.wheel_z),
      "hub",
      "hub-" .. i
    )
    add(
      axle:rotate(0, 0, yaw):translate(c.x, c.y, M.wheel_z),
      "link",
      "upright-" .. i
    )
  end

  -- Upper control arms, one per corner.
  local upper = suspension.upper_link(upper_link_width)
  local linkage = nil
  local function weld(solid)
    linkage = linkage and (linkage + solid) or solid
  end

  for _, x in ipairs { M.front_gearbox_x, M.back_gearbox_x } do
    weld(upper:translate(x, upper_link_left_y, upper_link_z))
    weld(upper:translate(x, upper_link_right_y, upper_link_z))
  end

  -- Drive shafts. Each is splayed outwards and cambered slightly.
  local shaft = suspension.wheel_shaft(wheel_shaft_width)
  local shafts = nil
  for _, side in ipairs { 1, -1 } do
    for _, front in ipairs { true, false } do
      local x = front and M.front_gearbox_x or M.back_gearbox_x
      local pitch = front and -34 or 34
      local placed = shaft
        :rotate(0, pitch, 0)
        :rotate(-7.5 * side, 0, 0)
        :translate(x, side * wheel_shaft_left_y, wheel_shaft_z)
      shafts = shafts and (shafts + placed) or placed
    end
  end

  -- Lower A-arms.
  local lower = suspension.lower_link(lower_link_width)
  for _, side in ipairs { 1, -1 } do
    -- 184 rather than -4 flips the arm over for the right-hand side.
    local roll = side == 1 and -4 or 184
    for _, x in ipairs {
      platform.front_suspension_connector_x,
      platform.back_suspension_connector_x,
    } do
      weld(lower:rotate(roll, 0, 0):translate(x, side * lower_link_left_y, lower_link_z))
    end
  end

  -- Steering / camber turnbuckles.
  local turnbuckle = suspension.link(link_width)
  for _, side in ipairs { 1, -1 } do
    for _, x in ipairs { link_front_x, link_back_x } do
      weld(
        turnbuckle
          :rotate(0, 0, side * 1.75)
          :translate(x, side * link_left_y, link_z)
      )
    end
  end

  add(linkage, "link", "suspension-linkage")
  add(shafts, "shaft", "drive-shafts")

  -- Shock absorbers: four coil-overs leaning in towards the centre line.
  local dampers, springs = nil, nil
  local shock_corners = {
    { x = shock_front_x, y = shock_left_y, rot = { 38, -5, 0 } },
    { x = shock_back_x, y = shock_left_y, rot = { -38, -3, 180 } },
    { x = shock_front_x, y = -shock_left_y, rot = { -38, -5, 0 } },
    { x = shock_back_x, y = -shock_left_y, rot = { 38, -3, 180 } },
  }

  local damper = shock.damper(shock_height)
  local spring = shock.spring(shock_height)

  for _, c in ipairs(shock_corners) do
    local place = function(solid)
      return solid
        :translate(0, 0, -shock_height / 2)
        :rotate(c.rot[1], c.rot[2], c.rot[3])
        :translate(c.x, c.y, shock_z)
    end
    local d, s = place(damper), place(spring)
    dampers = dampers and (dampers + d) or d
    springs = springs and (springs + s) or s
  end

  add(dampers, "damper", "shock-bodies")
  add(springs, "spring", "shock-springs")

  return out
end

return M
