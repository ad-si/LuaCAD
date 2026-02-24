require("luacad")

my_cube = cube { size = { 1, 2, 3 } }

function my_sphere(radius)
  return sphere({ r = 2 }):translate(5, 0, 0)
end

model = my_cube + my_sphere(2)

model:export("simple.scad")
