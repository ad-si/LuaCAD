require("lib_cad")

local model = cad.cube { { 60, 20, 10 }, center = true }
  + cad.cube({ { 30, 20, 10 }, center = true }):translate(5, 0, 10 - 0.001)
  + cad
    .cylinder({ h = 3, r = 8, center = true })
    :rotate(90, 0, 0)
    :translate(-20, -15, 0)
  + cad
    .cylinder({ h = 3, r = 8, center = true })
    :rotate(90, 0, 0)
    :translate(-20, 15, 0)
  + cad
    .cylinder({ h = 3, r = 8, center = true })
    :rotate(90, 0, 0)
    :translate(20, -15, 0)
  + cad
    .cylinder({ h = 3, r = 8, center = true })
    :rotate(90, 0, 0)
    :translate(20, 15, 0)
  + cad
    .cylinder({ h = 30, r = 2, center = true })
    :rotate(90, 0, 0)
    :translate(-20, 0, 0)
  + cad
    .cylinder({ h = 30, r = 2, center = true })
    :rotate(90, 0, 0)
    :translate(20, 0, 0)

model:export("car.scad")
