require("luacad")

local model = cube { { 10, 10, 10 }, center = true }
  - cylinder { h = 20, r = 3, center = true }

model:export("difference.scad")
