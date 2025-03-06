--[[--================================================================================
  Test file for testing simple CAD operations
--]]
--================================================================================

local luaunit = require("luaunit")
require("lib_cad")

TestSimple = {}

function TestSimple:setUp()
  -- Create temp directory if it doesn't exist
  os.execute("mkdir -p tests/temp")
end

function TestSimple:testCubeCreation()
  local cube = cad.cube { size = { 1, 2, 3 } }
  cube:export("temp/test_simple_cube.scad")

  -- Verify file was created
  local file = io.open("tests/temp/test_simple_cube.scad", "r")
  luaunit.assertNotNil(file, "SCAD file for cube was not created")
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

function TestSimple:testSphereCreation()
  local sphere = cad.sphere { r = 2 }
  sphere:export("temp/test_simple_sphere.scad")

  -- Verify file was created
  local file = io.open("tests/temp/test_simple_sphere.scad", "r")
  luaunit.assertNotNil(file, "SCAD file for sphere was not created")
  if file then
    local content = file:read("*all")
    file:close()
    luaunit.assertStrContains(
      content,
      "sphere",
      false,
      "Sphere command not found in generated SCAD file"
    )
  end
end

function TestSimple:testCylinderCreation()
  local files = {
    "temp/test_simple_cylinder_r.scad",
    "temp/test_simple_cylinder_d.scad",
    "temp/test_simple_cylinder_cone_r.scad",
    "temp/test_simple_cylinder_cone_d.scad",
    "temp/test_simple_cylinder_centered.scad",
  }

  -- Create the cylinders
  cad.cylinder({ h = 10, r = 5 }):export(files[1])
  cad.cylinder({ h = 10, d = 10 }):export(files[2])
  cad.cylinder({ h = 15, r1 = 5, r2 = 2 }):export(files[3])
  cad.cylinder({ h = 15, d1 = 10, d2 = 4 }):export(files[4])
  cad.cylinder({ h = 10, r = 5, center = true }):export(files[5])

  for _, filepath in ipairs(files) do
    local file = io.open("tests/" .. filepath, "r")
    luaunit.assertNotNil(file, "SCAD file " .. filepath .. " was not created")
    if file then
      file:close()
    end
  end
end

function TestSimple:testCombiningShapes()
  local cube = cad.cube { size = { 1, 2, 3 } }
  local sphere = cad.sphere { r = 2 }

  local model = cube + sphere
  model:export("temp/test_simple_combined.scad")

  -- Verify file was created
  local file = io.open("tests/temp/test_simple_combined.scad", "r")
  luaunit.assertNotNil(file, "SCAD file for combined shapes was not created")
  if file then
    file:close()
  end
end

function TestSimple:testShowOnlyModifier()
  -- Test the showOnly functionality in a more direct manner
  local obj = cad.cube { size = { 10, 10, 10 } }
  local obj_show = cad.s(obj)

  -- Basic test of showOnly property
  luaunit.assertTrue(obj_show.showOnly, "showOnly property not set correctly")
  luaunit.assertNotEquals(
    obj,
    obj_show,
    "Original and modified objects should be different"
  )
end

-- Run the tests
if not ... then
  os.exit(luaunit.LuaUnit.run())
end
