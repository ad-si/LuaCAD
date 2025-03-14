require("luacad")

-- This will cause an error due to non-string variable name
var(123, 10)

model = cube { size = { 3, 3, 5 } }
model:export("test.scad")
