-- MuSHR racecar -- the open-source robotic car from the UW Personal
-- Robotics Lab, ported to LuaCAD from the OpenSCAD original at
-- https://github.com/prl-mushr/mushr_cad (BSD-3-Clause).
--
--   luacad convert racecar.lua racecar.3mf --via-manifold
--   luacad render  racecar.lua racecar.png
--
-- Every part is its own coloured object, so the 3MF opens as an assembly
-- rather than one welded lump. See parts/palette.lua for the colours and
-- the surface materials (rubber tires, metal linkage -- best seen with
-- `luacad render --raytrace`).

package.path = "./?.lua;" .. package.path

local body = require("parts.body")
local chassis = require("parts.chassis")

local function finish(part)
  local solid = part.solid:color(part.color):name(part.name)
  return part.material and solid:material(part.material) or solid
end

-- The body is modelled around the crossbar; the chassis hangs below it.
for _, part in ipairs(body.parts()) do
  render(finish(part))
end

for _, part in ipairs(chassis.parts()) do
  render(
    finish(part):translate(body.chassis_x, body.chassis_y, body.chassis_z)
  )
end
