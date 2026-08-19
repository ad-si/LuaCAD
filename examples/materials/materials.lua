-- Surface materials: how each kind scatters light.
-- Best viewed with `luacad render --raytrace`, where metals reflect,
-- glass refracts, and emissive shapes cast light of their own.

local fn = 72 -- High segment count for smooth surfaces

local function ball(x, y)
  return sphere{r = 5, fn = fn}:translate(x, y, 5)
end

local floor = cube{70, 50, 2}
  :translate(-35, -25, -2)
  :color("#d9d9e0")
  :material("matte")

-- A 3 x 4 grid of spheres, one per material

-- Base kinds, front row (with the default color)
local matte = ball(-21, -14):material("matte")
local plastic = ball(-7, -14):material("plastic")
local metal = ball(7, -14):material("metal")
local glass = ball(21, -14):material("glass", {ior = 1.5})

-- Middle row: the light source and the first colored presets
local glow = ball(-21, 0)
  :color("#ffd66b")
  :material("emissive", {strength = 4})
local gold = ball(-7, 0):material("gold")
local copper = ball(7, 0):material("copper")
local steel = ball(21, 0):material("steel")

-- Back row: the remaining presets with built-in colors
local chrome = ball(-21, 14):material("chrome")
local rubber = ball(-7, 14):material("rubber")
local wood = ball(7, 14):material("wood")
local ivory = ball(21, 14):material("ivory")

-- The floor goes last: a union inherits its base color from the first
-- operand, and the uncolored balls should keep the default blue.
render(
  matte + plastic + metal + glass + glow
    + gold + copper + steel + chrome + rubber + wood + ivory
    + floor
)
