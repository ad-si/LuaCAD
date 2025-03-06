--[[--================================================================================
  Test file for longhole functions
--]]
--================================================================================

local luaunit = require("luaunit")
require("lib_cad")

TestLonghole = {}

function TestLonghole:setUp()
  -- Create temp directory if it doesn't exist
  os.execute("mkdir -p tests/temp")
end

function TestLonghole:testLongholeCreation()
  -- Test creating a longhole
  local obj = cad.longhole {
    points = { { 10, 20 }, { 30, 40 } },
    r = 5,
  }
  obj:export("temp/test_longhole.scad")

  -- Verify file was created
  local file = io.open("tests/temp/test_longhole.scad", "r")
  luaunit.assertNotNil(file, "SCAD file for longhole was not created")
  if file then
    local content = file:read("*all")
    file:close()
    luaunit.assertStrContains(
      content,
      "hull",
      "Hull command not found in generated SCAD file"
    )
    luaunit.assertStrContains(
      content,
      "circle",
      "Circle command not found in generated SCAD file"
    )
  end
end

function TestLonghole:testLongholeRelative()
  -- Test creating a relative longhole
  local obj = cad.longholerelative {
    point = { 5, 10 },
    delta = { 20, 15 },
    r = 3,
  }
  obj:export("temp/test_longhole_relative.scad")

  -- Verify file was created
  local file = io.open("tests/temp/test_longhole_relative.scad", "r")
  luaunit.assertNotNil(file, "SCAD file for relative longhole was not created")
  if file then
    local content = file:read("*all")
    file:close()
    luaunit.assertStrContains(
      content,
      "hull",
      "Hull command not found in generated SCAD file"
    )
    luaunit.assertStrContains(
      content,
      "circle",
      "Circle command not found in generated SCAD file"
    )
  end
end

-- Run all tests
if not ... then
  os.exit(luaunit.LuaUnit.run())
end
