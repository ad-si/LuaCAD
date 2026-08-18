-- Surface materials: how each kind scatters light.
-- Best viewed with `luacad render --raytrace`, where metals reflect,
-- glass refracts, and emissive shapes cast light of their own.

local fn = 72 -- High segment count for smooth surfaces

local function ball(x, y)
  return sphere{r = 5, fn = fn}:translate(x, y, 5)
end

local floor = cube{70, 32, 2}
  :translate(-35, -16, -2)
  :color("#d9d9e0")
  :material("matte")

-- Base kinds, front row (with the default color)
local matte = ball(-28, -8):material("matte")
local plastic = ball(-14, -8):material("plastic")
local metal = ball(0, -8):material("metal")
local glass = ball(14, -8):material("glass", {ior = 1.5})
local glow = ball(28, -8)
  :color("#ffd66b")
  :material("emissive", {strength = 4})

-- Presets with built-in colors, back row
local gold = ball(-28, 8):material("gold")
local copper = ball(-14, 8):material("copper")
local steel = ball(0, 8):material("steel")
local chrome = ball(14, 8):material("chrome")
local rubber = ball(28, 8):material("rubber")

-- The floor goes last: a union inherits its base color from the first
-- operand, and the uncolored balls should keep the default blue.
render(
  matte + plastic + metal + glass + glow
    + gold + copper + steel + chrome + rubber
    + floor
)
