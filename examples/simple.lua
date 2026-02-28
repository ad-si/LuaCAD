local my_cube = cube { size = { 1, 2, 3 } }

local function my_sphere(_radius)
  return sphere({ r = 2 }):translate(5, 0, 0)
end

local model = my_cube + my_sphere(2)

render(model)
