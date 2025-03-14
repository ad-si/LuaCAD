local luaunit = require("luaunit")
require("luascad")

TestProjection = {}

function TestProjection:setUp()
  -- Create temp directory if it doesn't exist
  os.execute("mkdir -p tests/temp")
end

function TestProjection:testSimpleProjection()
  -- Create a cube and project it
  local cube = cube { size = { 10, 10, 10 } }
  local projected = cube:projection()
  projected:export("temp/test_projection_simple.scad")

  -- Verify file was created
  local file = io.open("tests/temp/test_projection_simple.scad", "r")
  luaunit.assertNotNil(file, "SCAD file for simple projection was not created")
  if file then
    local content = file:read("*all")
    file:close()
    luaunit.assertStrContains(
      content,
      "projection",
      "Projection command not found in generated SCAD file"
    )
    luaunit.assertStrContains(
      content,
      "cut = false",
      "Default cut parameter not set to false"
    )
  end
end

function TestProjection:testCutProjection()
  -- Create a cube and project it with cut=true
  local cube = cube { size = { 10, 10, 10 } }
  local projected = cube:projection(true)
  projected:export("temp/test_projection_cut.scad")

  -- Verify file was created
  local file = io.open("tests/temp/test_projection_cut.scad", "r")
  luaunit.assertNotNil(file, "SCAD file for cut projection was not created")
  if file then
    local content = file:read("*all")
    file:close()
    luaunit.assertStrContains(
      content,
      "projection",
      "Projection command not found in generated SCAD file"
    )
    luaunit.assertStrContains(
      content,
      "cut = true",
      "Cut parameter not set to true"
    )
  end
end

function TestProjection:testComplexProjection()
  -- Create a more complex 3D shape (cylinder on top of cube) and project it
  local cube = cube { size = { 20, 20, 10 } }
  local cylinder = cylinder({ h = 15, r = 5 }):translate(10, 10, 10)
  local combined = cube:add(cylinder)
  local projected = combined:projection()
  projected:export("temp/test_projection_complex.scad")

  -- Verify file was created
  local file = io.open("tests/temp/test_projection_complex.scad", "r")
  luaunit.assertNotNil(file, "SCAD file for complex projection was not created")
  if file then
    local content = file:read("*all")
    file:close()
    luaunit.assertStrContains(
      content,
      "projection",
      "Projection command not found in generated SCAD file"
    )
    -- Verify both shapes are included
    luaunit.assertStrContains(
      content,
      "cube",
      "Cube command not found in generated SCAD file"
    )
    luaunit.assertStrContains(
      content,
      "cylinder",
      "Cylinder command not found in generated SCAD file"
    )
  end
end

-- Run the tests
if not ... then
  os.exit(luaunit.LuaUnit.run())
end
