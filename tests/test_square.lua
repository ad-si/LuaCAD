--[[--================================================================================
  Test file for square/rect functions
--]]
--================================================================================

local luaunit = require("luaunit")
require("luacad")

TestSquare = {}

function TestSquare:setUp()
  -- Create temp directory if it doesn't exist
  os.execute("mkdir -p tests/temp")
end

function TestSquare:testSquareCreation()
  -- Test creating a square with the new signature
  local obj = square { size = { 10, 20 }, center = true }
  obj:export("temp/test_square.scad")

  -- Verify file was created
  local file = io.open("tests/temp/test_square.scad", "r")
  luaunit.assertNotNil(file, "SCAD file for square was not created")
  if file then
    local content = file:read("*all")
    file:close()
    luaunit.assertStrContains(
      content,
      "square",
      "Square command not found in generated SCAD file"
    )
    luaunit.assertStrContains(
      content,
      "translate",
      "Translate command not found in generated SCAD file"
    )
  end
end

function TestSquare:testSquareWithSingleSize()
  -- Test creating a square with just one size value (should make a square)
  local obj = square { size = { 15 }, center = false }
  obj:export("temp/test_square_single.scad")

  -- Verify file was created
  local file = io.open("tests/temp/test_square_single.scad", "r")
  luaunit.assertNotNil(
    file,
    "SCAD file for single-sized square was not created"
  )
  if file then
    local content = file:read("*all")
    file:close()
    luaunit.assertStrContains(
      content,
      "square",
      "Square command not found in generated SCAD file"
    )
  end
end

function TestSquare:testRectAlias()
  -- Test that rect is working the same way
  local obj = rect { size = { 30, 40 }, center = true }
  obj:export("temp/test_rect.scad")

  -- Verify file was created
  local file = io.open("tests/temp/test_rect.scad", "r")
  luaunit.assertNotNil(file, "SCAD file for rect was not created")
  if file then
    local content = file:read("*all")
    file:close()
    luaunit.assertStrContains(
      content,
      "square",
      "Square command not found in generated SCAD file"
    )
    luaunit.assertStrContains(
      content,
      "translate",
      "Translate command not found in generated SCAD file"
    )
  end
end

-- Run all tests
if not ... then
  os.exit(luaunit.LuaUnit.run())
end
