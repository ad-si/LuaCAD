
-- draw a simple diabolo not with half spheres but zylinders at the ends

require "lib_cad"

local height = 60/2
local radius_bottom = 50/2
local radius_middle = 20/2
local thickness = 10/2
local frags = 6

if #arg > 0 then
  height = arg[1]
  radius_bottom = arg[2]
  radius_middle = arg[3]
  thickness = arg[4]
  frags = arg[5]
end

-- calc angle
local h = height
local alpha = math.atan((h/2)/(radius_bottom-radius_middle))
local alpha_deg = tonumber(string.format("%.2f",math.deg(alpha)))

local cad_file = "temp/diabolo_cylindric_[h="..height..",r1="..radius_bottom..",r2="..radius_middle..",d="..thickness..",frag="..frags..",angle="..alpha_deg.."].stl"

print("Output = "..cad_file)

local cylinder = (cad.cylinder(0,0,0, h/2, radius_bottom, radius_middle)
  - cad.cylinder(0,0,0, h/2, radius_bottom-thickness, radius_middle-thickness))
  + (cad.cylinder(0,0,h/2, h/2, radius_middle, radius_bottom)
  - cad.cylinder(0,0,h/2, h/2, radius_middle-thickness, radius_bottom-thickness))

-- cylinder:set_circle_fragments(frags)

cylinder:export(cad_file)
