require("luascad")

-- This will cause an error due to invalid variable name
var("1invalid", 10)

model = cube { size = { 3, 3, 5 } }
model:export("test.scad")
