require("lib_cad")

local model = cad.cube { { 10, 10, 10 }, center = true }
  - cad.cylinder { h = 20, r = 3, center = true }

model:export("difference.scad")
