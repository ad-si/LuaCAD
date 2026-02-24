require("luacad")

function create_box(width, depth, height, thickness)
  local outer_shell = cube(width, depth, height)
  local inner_cavity = cube(
    width - thickness * 2,
    depth - thickness * 2,
    height -- `thickness` higher than top surface to avoid z-fighting
  ):translate(thickness, thickness, thickness)
  return outer_shell - inner_cavity
end

create_box(30, 20, 15, 2):export("box.scad")
