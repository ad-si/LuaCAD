local luaunit = require('luaunit')
require "lib_cad"

TestCube = {}

function TestCube:setUp()
  -- Create temp directory if it doesn't exist
  os.execute("mkdir -p temp")
end

function TestCube:testCubeCreation()
  local cube = cad.cube{size={3, 3, 3}}
  cube:export("temp/test_cube_basic.scad")
  
  -- Verify file was created
  local file = io.open("temp/test_cube_basic.scad", "r")
  luaunit.assertNotNil(file, "SCAD file for basic cube was not created")
  if file then
    local content = file:read("*all")
    file:close()
    luaunit.assertStrContains(content, "cube", "Cube command not found in generated SCAD file")
  end
end

function TestCube:testCubeWithCustomSize()
  local cube = cad.cube{size={1, 2, 3}}
  cube:export("temp/test_cube_custom.scad")
  
  -- Verify file was created
  local file = io.open("temp/test_cube_custom.scad", "r")
  luaunit.assertNotNil(file, "SCAD file for custom-sized cube was not created")
  if file then
    local content = file:read("*all")
    file:close()
    luaunit.assertStrContains(content, "cube", "Cube command not found in generated SCAD file")
  end
end

function TestCube:testCubeWithCenter()
  local cube = cad.cube{size={2, 2, 2}, center=true}
  cube:export("temp/test_cube_centered.scad")
  
  -- Verify file was created
  local file = io.open("temp/test_cube_centered.scad", "r")
  luaunit.assertNotNil(file, "SCAD file for centered cube was not created")
  if file then
    local content = file:read("*all")
    file:close()
    luaunit.assertStrContains(content, "cube", "Cube command not found in generated SCAD file")
    -- The implementation might handle centering with translate instead of the center parameter
    -- So we'll check that either the file contains "center" or has a translation that centers the cube
    if not string.find(content, "center") then
      luaunit.assertStrContains(content, "translate", "Neither center nor translate found in generated SCAD file")
    end
  end
end

-- Run the tests
if not ... then
  os.exit(luaunit.LuaUnit.run())
end
