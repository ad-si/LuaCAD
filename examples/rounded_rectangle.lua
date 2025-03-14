require("luacad")

function rounded_rect(width, height, radius)
  local model = circle({ r = radius }):translate(radius, radius, 0)
    + circle({ r = radius }):translate(width - radius, radius, 0)
    + circle({ r = radius }):translate(radius, height - radius, 0)
    + circle({ r = radius }):translate(width - radius, height - radius, 0)
  return model:hull()
end

rounded_rect(20, 10, 2) --
  :linearextrude(5)
  :export("rounded_rectangle.scad")
