require("lib_cad")

-- This will cause an error due to non-string variable name
cad.var(123, 10)

model = cad.cube { size = { 3, 3, 5 } }
model:export("test.scad")
