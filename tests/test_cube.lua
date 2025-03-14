local luaunit = require("luaunit")
require("luascad")

TestCube = {}

function TestCube:setUp()
  -- Create temp directory if it doesn't exist
  os.execute("mkdir -p tests/temp")
end

function TestCube:testCubeCreation()
  local cube = cube { size = { 3, 3, 3 } }
  cube:export("temp/test_cube_basic.scad")

  -- Verify file was created
  local file = io.open("tests/temp/test_cube_basic.scad", "r")
  luaunit.assertNotNil(file, "SCAD file for basic cube was not created")
  if file then
    local content = file:read("*all")
    file:close()
    luaunit.assertStrContains(
      content,
      "cube",
      false,
      "Cube command not found in generated SCAD file"
    )
  end
end

function TestCube:testCubeWithCustomSize()
  local cube = cube { size = { 1, 2, 3 } }
  cube:export("temp/test_cube_custom.scad")

  -- Verify file was created
  local file = io.open("tests/temp/test_cube_custom.scad", "r")
  luaunit.assertNotNil(file, "SCAD file for custom-sized cube was not created")
  if file then
    local content = file:read("*all")
    file:close()
    luaunit.assertStrContains(
      content,
      "cube",
      false,
      "Cube command not found in generated SCAD file"
    )
  end
end

function TestCube:testCubeWithCenter()
  local cube = cube { size = { 2, 2, 2 }, center = true }
  cube:export("temp/test_cube_centered.scad")

  -- Verify file was created
  local file = io.open("tests/temp/test_cube_centered.scad", "r")
  luaunit.assertNotNil(file, "SCAD file for centered cube was not created")
  if file then
    local content = file:read("*all")
    file:close()
    luaunit.assertStrContains(
      content,
      "cube",
      false,
      "Cube command not found in generated SCAD file"
    )
    -- The implementation might handle centering with translate instead of the center parameter
    -- So we'll check that either the file contains "center" or has a translation that centers the cube
    if not string.find(content, "center") then
      luaunit.assertStrContains(
        content,
        "translate",
        false,
        "Neither center nor translate found in generated SCAD file"
      )
    end
  end
end

function TestCube:testCubeWithArraySyntax()
  local cube = cube { 3, 3, 5 }
  cube:export("temp/test_cube_array_syntax.scad")

  -- Verify file was created
  local file = io.open("tests/temp/test_cube_array_syntax.scad", "r")
  luaunit.assertNotNil(file, "SCAD file for array syntax cube was not created")
  if file then
    local content = file:read("*all")
    file:close()
    -- Check for the dimensions in the SCAD output
    luaunit.assertStrContains(
      content,
      "cube([3, 3, 5])",
      false,
      "Cube with correct dimensions not found in SCAD file"
    )
  end
end

function TestCube:testCubeWithArraySyntaxAndCenter()
  local cube = cube { 4, 4, 6, center = true }
  cube:export("temp/test_cube_array_syntax_centered.scad")

  -- Verify file was created
  local file = io.open("tests/temp/test_cube_array_syntax_centered.scad", "r")
  luaunit.assertNotNil(
    file,
    "SCAD file for centered array syntax cube was not created"
  )
  if file then
    local content = file:read("*all")
    file:close()
    -- Check for the dimensions in the SCAD output
    luaunit.assertStrContains(
      content,
      "cube([4, 4, 6])",
      false,
      "Cube with correct dimensions not found in SCAD file"
    )
    -- The implementation might handle centering with translate instead of the center parameter
    -- So we'll check that the file contains a translation that centers the cube
    luaunit.assertStrContains(
      content,
      "translate([-2.0, -2.0, -3.0])",
      false,
      "Translation for centering not found in generated SCAD file"
    )
  end
end

-- Run the tests
if not ... then
  os.exit(luaunit.LuaUnit.run())
end
