require("luacad")

function gear(num_teeth, height, radius)
  local model = cylinder { h = height, r = radius * 0.7 }

  -- Create teeth
  for i = 1, num_teeth do
    local angle = i * (360 / num_teeth)
    model = model
      + cad
        .cube(radius * 0.3, radius * 0.2, height)
        :translate(radius * 0.7, 0, 0)
        :rotate(0, 0, angle)
  end

  return model
end

gear(8, 5, 10):export("gear.scad")
