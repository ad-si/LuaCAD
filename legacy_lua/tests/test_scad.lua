local luaunit = require("luaunit")
require("luacad")

TestScad = {}

function TestScad:setUp()
  -- Create temp directory if it doesn't exist
  os.execute("mkdir -p tests/temp")
end

-- Test the basic scad function
function TestScad:testBasicScad()
  -- Create a scad object with custom code
  local custom = scad("cube([5, 5, 5]);")

  -- Export the model
  custom:export("temp/test_scad_basic.scad")

  -- Verify the file exists
  local file = io.open("tests/temp/test_scad_basic.scad", "r")
  luaunit.assertNotNil(file, "SCAD file was not created")

  -- Read the content and verify it contains our custom code
  local content = file:read("*all")
  file:close()

  luaunit.assertStrContains(
    content,
    "cube([5, 5, 5])",
    false,
    "SCAD file does not contain the custom code"
  )
end

-- Test combining scad with other objects
function TestScad:testCombinedScad()
  -- Create a regular object and a scad object
  local cube_obj = cube { size = { 3, 3, 3 } }
  local custom = scad("sphere(r=2);")

  -- Combine them
  local combined = cube_obj + custom

  -- Export the combined model
  combined:export("temp/test_scad_combined.scad")

  -- Verify the file exists
  local file = io.open("tests/temp/test_scad_combined.scad", "r")
  luaunit.assertNotNil(file, "SCAD file was not created")

  -- Read the content and verify it contains both objects
  local content = file:read("*all")
  file:close()

  luaunit.assertStrContains(
    content,
    "cube",
    false,
    "SCAD file does not contain the cube"
  )
  luaunit.assertStrContains(
    content,
    "sphere(r=2)",
    false,
    "SCAD file does not contain the custom sphere code"
  )
end

-- Test using scad with transformations
function TestScad:testScadWithTransform()
  -- Create a scad object with transformation
  local custom = scad("cylinder(h=5, r=2);"):translate(10, 0, 0)

  -- Export the model
  custom:export("temp/test_scad_transform.scad")

  -- Verify the file exists
  local file = io.open("tests/temp/test_scad_transform.scad", "r")
  luaunit.assertNotNil(file, "SCAD file was not created")

  -- Read the content and verify it contains the transformed code
  local content = file:read("*all")
  file:close()

  luaunit.assertStrContains(
    content,
    "translate",
    false,
    "SCAD file does not contain the translation"
  )
  luaunit.assertStrContains(
    content,
    "cylinder",
    false,
    "SCAD file does not contain the cylinder"
  )
end

-- Run the tests
if not ... then
  os.exit(luaunit.LuaUnit.run())
end
