--[[--================================================================================
  Test file for circle function
--]]
--================================================================================

local luaunit = require("luaunit")
require("lib_cad")

TestCircle = {}

function TestCircle:setUp()
  -- Create temp directory if it doesn't exist
  os.execute("mkdir -p tests/temp")
end

function TestCircle:testCircleCreationWithRadius()
  -- Test creating a circle with radius parameter
  local obj = cad.circle { r = 10 }
  obj:export("temp/test_circle_radius.scad")

  -- Verify file was created
  local file = io.open("tests/temp/test_circle_radius.scad", "r")
  luaunit.assertNotNil(file, "SCAD file for circle was not created")
  if file then
    local content = file:read("*all")
    file:close()
    luaunit.assertStrContains(
      content,
      "circle",
      "Circle command not found in generated SCAD file"
    )
    luaunit.assertStrContains(
      content,
      "r = 10",
      "Radius parameter not found in generated SCAD file"
    )
  end
end

function TestCircle:testCircleCreationWithDiameter()
  -- Test creating a circle with diameter parameter
  local obj = cad.circle { d = 20 }
  obj:export("temp/test_circle_diameter.scad")

  -- Verify file was created
  local file = io.open("tests/temp/test_circle_diameter.scad", "r")
  luaunit.assertNotNil(file, "SCAD file for circle was not created")
  if file then
    local content = file:read("*all")
    file:close()
    luaunit.assertStrContains(
      content,
      "circle",
      "Circle command not found in generated SCAD file"
    )
    luaunit.assertStrContains(
      content,
      "d = 20",
      "Diameter parameter not found in generated SCAD file"
    )
  end
end

-- Run all tests
if not ... then
  os.exit(luaunit.LuaUnit.run())
end
