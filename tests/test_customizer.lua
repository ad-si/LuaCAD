require("lib_cad")

cad.var("x", 1)

model = cad.cube { size = { 3, 3, 5 } }
  + cad.cube { size = { 1, 2, 3 }, center = true }
  + cad.sphere({ r = "x" }):translate(8, 8, 0) -- Direct parameters

model:export("test.scad")
