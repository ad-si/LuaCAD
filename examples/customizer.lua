require("lib_cad")

local r = cad.var("Radius of sphere", 9)
local t = cad.var("Translation of sphere", 3)
local h = cad.var("Height of cube", 4)

local rNew = r + 5 + h - 2

local model = cad.cube { size = { 3, 3, 5 } }
  + cad.cube { size = { 1, 2, 3 }, center = true }
  + cad.sphere({ r = r }):translate(t, t, t)
  + cad.sphere { r = rNew }

model:export("test.scad")
