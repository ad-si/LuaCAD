function gear(num_teeth, height, radius)
  local model = cylinder { h = height, r = radius * 0.7 }
  local tooth_length = radius * 0.3;
  local tooth_width = radius * 0.2;

  -- Create teeth
  for i = 1, num_teeth do
    local angle = i * (360 / num_teeth)
    model = model
      + cube(tooth_length * 1.2, tooth_width, height)
          :translate(radius - (tooth_length * 1.2), -tooth_width/2, 0)
          :rotate(0, 0, angle)
  end

  return model
end

render(gear(8, 5, 10))
