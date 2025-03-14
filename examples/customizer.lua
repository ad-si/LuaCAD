require("luascad")

local r = var("Radius of sphere", 9)
local t = var("Translation of sphere", 3)
local h = var("Height of cube", 4)

local rNew = r + 5 + h - 2

local model = cube { size = { 3, 3, 5 } }
  + cube { size = { 1, 2, 3 }, center = true }
  + sphere({ r = r }):translate(t, t, t)
  + sphere { r = rNew }

model:export("test.scad")
