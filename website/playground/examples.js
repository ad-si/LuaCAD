// Starter scripts for the playground. Kept short on purpose: each one should
// fit on screen next to its model and show a single idea.

export const EXAMPLES = [
  {
    name: "Boolean operations",
    code: `-- Union, intersection and difference, side by side

local union =
  (cube { 15, 15, 15, center = true } + sphere { r = 10 })
    :translate(-24, 0, 0)

local intersection =
  cube { 15, 15, 15, center = true }
    :intersect(sphere { r = 10 })

local difference =
  (cube { 15, 15, 15, center = true } - sphere { r = 10 })
    :translate(24, 0, 0)

render(union + intersection + difference)
`,
  },
  {
    name: "Parametric gear",
    code: `-- A gear built by adding one tooth at a time

function gear(num_teeth, height, radius)
  local model = cylinder { h = height, r = radius * 0.7 }
  local tooth_length = radius * 0.3
  local tooth_width = radius * 0.2

  for i = 1, num_teeth do
    local angle = i * (360 / num_teeth)
    model = model
      + cube(tooth_length * 1.2, tooth_width, height)
          :translate(radius - (tooth_length * 1.2), -tooth_width / 2, 0)
          :rotate(0, 0, angle)
  end

  return model
end

render(gear(12, 5, 20))
`,
  },
  {
    name: "Colored parts",
    code: `-- Every rendered value is its own part, with its own color

local base = cube { 40, 40, 4, center = true }
  :color("steelblue")
  :name("Base")

local post = cylinder { h = 30, r = 4 }
  :translate(0, 0, 2)
  :color("orange")
  :name("Post")

local cap = sphere { r = 7 }
  :translate(0, 0, 32)
  :color("crimson")
  :name("Cap")

render(base)
render(post)
render(cap)
`,
  },
  {
    name: "Extruded outline",
    code: `-- 2D first, then extrude — the usual way to build a plate

local outline = square { 60, 40, center = true }
  - circle { r = 6 }:translate(-20, -10, 0)
  - circle { r = 6 }:translate(20, -10, 0)
  - circle { r = 6 }:translate(-20, 10, 0)
  - circle { r = 6 }:translate(20, 10, 0)

render(outline:linear_extrude { height = 5 })
`,
  },
  {
    name: "2D outline",
    code: `-- A 2D shape is output in its own right: no extrusion needed.
-- It draws flat here and exports to .scad as the calls that made it.
-- Mesh formats need a solid, so add :linear_extrude(3) for those.

local plate = square { 80, 50, center = true }
  - circle { r = 10 }

for _, x in ipairs({ -30, 30 }) do
  for _, y in ipairs({ -17, 17 }) do
    plate = plate - circle { r = 3 }:translate(x, y)
  end
end

render(plate:color("steelblue"))
`,
  },
  {
    name: "Loops and math",
    code: `-- A spiral of cubes, because a script can do arithmetic

local model = nil

for i = 0, 60 do
  local angle = i * 12
  local part = cube { 4, 4, 4, center = true }
    :translate(20 + i * 0.4, 0, i * 0.8)
    :rotate(0, 0, angle)
  model = model and (model + part) or part
end

render(model)
`,
  },
]
