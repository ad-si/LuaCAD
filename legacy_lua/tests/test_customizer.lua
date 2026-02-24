require("luacad")

var("x", 1)

model = cube { size = { 3, 3, 5 } }
  + cube { size = { 1, 2, 3 }, center = true }
  + sphere({ r = "x" }):translate(8, 8, 0) -- Direct parameters

model:export("test.scad")
