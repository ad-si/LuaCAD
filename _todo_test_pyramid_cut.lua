
-- simple pyramid
-- which is cut from each sides and the bottom, side_inner is the pyramids cut side when using the pyramids_innercircumcenter not the bottom area
-- and actual pyramid has a side length of 440 and a height of 280, or (110/70)

--[[-----------------------------------
  Author: Michael Lutz
  Licensed under the same terms as Lua itself
--]]-----------------------------------

require "lib_cad"

-- input variables
local side = 110
local height = 80 -- change to 80 for 45 degree angle, original pyramid would be 70
local side_inner = 70

-- if command line args given
if arg and (#arg > 0) then
  side = arg[1]
  height = arg[2]
  side_inner = arg[3]
end

print("Pyramid Side: "..side)
print("Pyramid Height: "..height)
print("Pyramid Side Cut: "..side_inner)

-- Do some calculation
-- calc ration and height_inner
local ratio = side/height
local height_inner = side_inner/ratio

-- calc side angle
local cross = math.sqrt((side*side)+(side*side))
local angle = math.atan((height*2)/cross)
local angle_deg = tonumber(string.format("%.2f",math.deg(angle)))
print("Max Angle = "..angle_deg.." deg")

-- output file
local cad_file = "Pyramid_Cut_["..side..","..height..","..side_inner..","..angle_deg.."-deg].stl"
print("Output file: "..cad_file)

-- calc distance of second pyramid
local d_x = (side-side_inner)/2
--print("d_x = "..d_x)
local cc_inner = geometry.get_pyramid_innercircumcenter(side_inner, height_inner)
local cc = geometry.get_pyramid_innercircumcenter(side, height)
--print("cc = "..cc)
local d_h = cc-cc_inner
--print("d_h: "..d_h)

-- calc cut pyramid height + d_h
-- get resulting bottom cut length
local cut_h = height_inner + d_h
local cut_side = cut_h*ratio
local d_x_cut = (side-cut_side)/2
--print("cut Pyramid(side, height): "..cut_side..", "..cut_h)

-- Create the Pyramid

local pyr = cad.pyramid(0,0,0, side, height)

local pyr_cut_1 = cad.pyramid(d_x_cut,d_x_cut,0, cut_side, cut_h)

local pyr_cut_2 = cad.polygon(0,0, {{0,0},{side_inner,0},{side_inner/2,height_inner}})
  :linear_extrude(side)
  :rotate(0,0,0, 90,0,0)
  :translate(d_x,side,d_h)
local pyr_cut_3 = cad.polygon(0,0, {{0,0},{height_inner,side_inner/2},{0,side_inner}})
  :linear_extrude(side)
  :rotate(0,0,0, 0,-90,0)
  :translate(side,d_x,d_h)

local pyr_cut = pyr - (pyr_cut_1 + pyr_cut_2 + pyr_cut_3)

pyr_cut:export(cad_file)

pyr_cut:export(cad_file..".obj", {128,240,5})
