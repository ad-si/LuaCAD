require("luacad")

-- Create and print some basic shapes
print("Basic shapes:")
print("- Cube: " .. tostring(cube(10, 10, 10)))
print("- Sphere: " .. tostring(sphere { r = 5 }))
print("- Cylinder: " .. tostring(cylinder { h = 10, r = 5 }))

-- Cylinders with different parameters
print("\nCylinder variations:")
print("- Standard cylinder: " .. tostring(cylinder { h = 10, r = 5 }))
print("- Cylinder with diameter: " .. tostring(cylinder { h = 10, d = 10 }))
print("- Cone (r1, r2): " .. tostring(cylinder { h = 10, r1 = 5, r2 = 2 }))
print("- Cone (d1, d2): " .. tostring(cylinder { h = 10, d1 = 10, d2 = 4 }))
print(
  "- Centered cylinder: " .. tostring(cylinder { h = 10, r = 5, center = true })
)

-- Colors
print("\nColored objects:")
local cube = cube(10, 10, 10)
local sphere = sphere { r = 7 }
print("- Red cube: " .. tostring(cube:setcolor("red")))
print("- Blue sphere: " .. tostring(sphere:setcolor("blue")))
print(
  "- Green cylinder: "
    .. tostring(cylinder({ h = 10, r = 5 }):setcolor("green"))
)

-- TODO: Transformations
-- print("\nTransformations:")
-- local box = cube(10, 10, 10)
-- print("- Original: " .. tostring(box))
-- print("- Translated: " .. tostring(box:translate(5, 5, 0)))
-- print("- Scaled: " .. tostring(box:scale(2, 1, 1)))
-- print("- Rotated: " .. tostring(box:rotate(0, 0, 45)))

-- TODO: CSG Operations
-- print("\nCSG Operations:")
-- local cube = cube(10, 10, 10)
-- local sphere = sphere { r = 7 }
-- print("- Union: " .. tostring(cube + sphere))
-- print("- Difference: " .. tostring(cube - sphere))
