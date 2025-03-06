require("lib_cad")

-- This will cause an error due to invalid variable name
cad.var("1invalid", 10)

model = cad.cube { size = { 3, 3, 5 } }
model:export("test.scad")
