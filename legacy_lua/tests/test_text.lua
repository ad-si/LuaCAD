--[[--================================================================================
  Test file for text function
--]]
--================================================================================

local luaunit = require("luaunit")
require("luacad")

TestText = {}

function TestText:setUp()
  -- Create temp directory if it doesn't exist
  os.execute("mkdir -p tests/temp")
end

function TestText:testTextCreationWithDefaultSpacing()
  -- Test creating text with default spacing (should output spacing=1)
  local obj = text(0, 0, 0, "Hello")
  obj:export("temp/test_text_default_spacing.scad")

  local file = io.open("tests/temp/test_text_default_spacing.scad", "r")
  luaunit.assertNotNil(file, "SCAD file for text was not created")
  if file then
    local content = file:read("*all")
    file:close()
    luaunit.assertStrContains(
      content,
      "text(",
      "Text command not found in generated SCAD file"
    )
    luaunit.assertStrContains(
      content,
      "spacing=1",
      "Default spacing parameter not found in generated SCAD file"
    )
    luaunit.assertStrContains(
      content,
      "Hello",
      "Text content not found in generated SCAD file"
    )
  end
end

function TestText:testTextCreationWithCustomSpacing()
  -- Test creating text with custom spacing
  local obj = text(0, 0, 0, "Test", 10, "Arial", nil, "left", "baseline", 1.5)
  obj:export("temp/test_text_custom_spacing.scad")

  local file = io.open("tests/temp/test_text_custom_spacing.scad", "r")
  luaunit.assertNotNil(file, "SCAD file for text was not created")
  if file then
    local content = file:read("*all")
    file:close()
    luaunit.assertStrContains(
      content,
      "text(",
      "Text command not found in generated SCAD file"
    )
    luaunit.assertStrContains(
      content,
      "spacing=1.5",
      "Custom spacing parameter not found in generated SCAD file"
    )
  end
end

-- Run all tests
if not ... then
  os.exit(luaunit.LuaUnit.run())
end
