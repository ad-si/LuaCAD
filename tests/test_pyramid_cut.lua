-- simple pyramid
-- which is cut from each sides and the bottom, side_inner is the pyramids cut side when using the pyramids_innercircumcenter not the bottom area
-- and actual pyramid has a side length of 440 and a height of 280, or (110/70)

require("luacad")
local luaunit = require("luaunit")

TestPyramidCut = {}

function TestPyramidCut:testPyramidCut()
  -- Create the test directory if it doesn't exist
  os.execute("mkdir -p tests/temp")

  -- input variables
  local side = 110
  local height = 80 -- change to 80 for 45 degree angle, original pyramid would be 70
  local side_inner = 70

  -- Do some calculation
  -- calc ration and height_inner
  local ratio = side / height
  local height_inner = side_inner / ratio

  -- calc side angle
  local cross = math.sqrt((side * side) + (side * side))
  local angle = math.atan((height * 2) / cross)
  local angle_deg = tonumber(string.format("%.2f", math.deg(angle)))

  -- output file
  local cad_file = "temp/pyramid_cut_["
    .. side
    .. ","
    .. height
    .. ","
    .. side_inner
    .. ","
    .. angle_deg
    .. "-deg].stl"

  -- calc distance of second pyramid
  local d_x = (side - side_inner) / 2
  local cc_inner =
    geometry.getpyramidinnercircumcenter(side_inner, height_inner)
  local cc = geometry.getpyramidcircumcenter(side, height)
  local d_h = cc - cc_inner

  -- calc cut pyramid height + d_h
  -- get resulting bottom cut length
  local cut_h = height_inner + d_h
  local cut_side = cut_h * ratio
  local d_x_cut = (side - cut_side) / 2

  -- Create the Pyramid
  local pyr = pyramid(0, 0, 0, side, height)

  local pyr_cut_1 = pyramid(d_x_cut, d_x_cut, 0, cut_side, cut_h)

  local pyr_cut_2 = cad
    .polygon({
      points = { { 0, 0 }, { side_inner, 0 }, { side_inner / 2, height_inner } },
    })
    :linear_extrude(side)
    :rotate(0, 0, 0, 90, 0, 0)
    :translate(d_x, side, d_h)
  local pyr_cut_3 = cad
    .polygon({
      points = { { 0, 0 }, { height_inner, side_inner / 2 }, { 0, side_inner } },
    })
    :linear_extrude(side)
    :rotate(0, 0, 0, 0, -90, 0)
    :translate(side, d_x, d_h)

  local pyr_cut = pyr - (pyr_cut_1 + pyr_cut_2 + pyr_cut_3)

  pyr_cut:export(cad_file)

  -- Verify file exists
  local file = io.open("tests/" .. cad_file)
  luaunit.assertNotNil(file, "STL file for pyramid was not created")
  if file then
    file:close()
  end
end

-- Run tests
if not ... then
  os.exit(luaunit.LuaUnit.run())
end
