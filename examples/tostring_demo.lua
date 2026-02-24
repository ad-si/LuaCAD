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
local my_cube = cube(10, 10, 10)
local my_sphere = sphere { r = 7 }
print("- Red cube: " .. tostring(my_cube:setcolor("red")))
print("- Blue sphere: " .. tostring(my_sphere:setcolor("blue")))
print(
  "- Green cylinder: "
    .. tostring(cylinder({ h = 10, r = 5 }):setcolor("green"))
)

-- Render the colored objects
render(my_cube:setcolor("red") + my_sphere:setcolor("blue"))
