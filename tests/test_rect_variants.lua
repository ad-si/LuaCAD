--[[--================================================================================
  Test file for rectround and rectchamfer functions
--]]
--================================================================================

local luaunit = require("luaunit")
require("luascad")

TestRectVariants = {}

function TestRectVariants:setUp()
  -- Create temp directory if it doesn't exist
  os.execute("mkdir -p tests/temp")
end

function TestRectVariants:testRectroundCreation()
  -- Test creating a rounded rectangle
  local obj = rectround {
    size = { 30, 20 },
    radius = 5,
    center = true,
  }
  obj:export("temp/test_rectround.scad")

  -- Verify file was created
  local file = io.open("tests/temp/test_rectround.scad", "r")
  luaunit.assertNotNil(file, "SCAD file for rounded rectangle was not created")
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

function TestRectVariants:testRectchamferCreation()
  -- Test creating a chamfered rectangle
  local obj = rectchamfer {
    size = { 40, 25 },
    chamfer = 8,
    center = false,
  }
  obj:export("temp/test_rectchamfer.scad")

  -- Verify file was created
  local file = io.open("tests/temp/test_rectchamfer.scad", "r")
  luaunit.assertNotNil(
    file,
    "SCAD file for chamfered rectangle was not created"
  )
  if file then
    local content = file:read("*all")
    file:close()
    luaunit.assertStrContains(
      content,
      "polygon",
      "Polygon command not found in generated SCAD file"
    )
  end
end

-- Run all tests
if not ... then
  os.exit(luaunit.LuaUnit.run())
end
