local luaunit = require("luaunit")
require("luascad")

TestMinkowski = {}

function TestMinkowski:setUp()
  -- Create temp directory if it doesn't exist
  os.execute("mkdir -p tests/temp")
end

function TestMinkowski:testMinkowskiBasic()
  -- Create a basic minkowski example: cube + sphere
  cube({ size = { 10, 10, 10 }, center = true })
    :add(sphere { r = 2 })
    :minkowski()
    :export("temp/test_minkowski_basic.scad")

  -- Verify file was created
  local file = io.open("tests/temp/test_minkowski_basic.scad", "r")
  luaunit.assertNotNil(file, "SCAD file for basic minkowski was not created")
  if file then
    local content = file:read("*all")
    file:close()
    luaunit.assertStrContains(
      content,
      "minkowski",
      "Minkowski command not found in generated SCAD file"
    )
    luaunit.assertStrContains(
      content,
      "cube",
      "Cube command not found in generated SCAD file"
    )
    luaunit.assertStrContains(
      content,
      "sphere",
      "Sphere command not found in generated SCAD file"
    )
  end
end

function TestMinkowski:test2DMinkowski()
  -- Create a 2D minkowski example: square + circle
  square({ size = { 10, 10 }, center = true })
    :add(circle { r = 2 })
    :minkowski()
    :export("temp/test_minkowski_2d.scad")

  -- Verify file was created
  local file = io.open("tests/temp/test_minkowski_2d.scad", "r")
  luaunit.assertNotNil(file, "SCAD file for 2D minkowski was not created")
  if file then
    local content = file:read("*all")
    file:close()
    luaunit.assertStrContains(
      content,
      "minkowski",
      "Minkowski command not found in generated SCAD file"
    )
    luaunit.assertStrContains(
      content,
      "square",
      "Square command not found in generated SCAD file"
    )
    luaunit.assertStrContains(
      content,
      "circle",
      "Circle command not found in generated SCAD file"
    )
  end
end

function TestMinkowski:testMinkowskiRounded()
  -- Create a rounded cube using minkowski
  cube({ size = { 10, 10, 10 }, center = true })
    :add(sphere { r = 1 })
    :minkowski()
    :export("temp/test_minkowski_rounded.scad")

  -- Verify file was created
  local file = io.open("tests/temp/test_minkowski_rounded.scad", "r")
  luaunit.assertNotNil(file, "SCAD file for rounded cube was not created")
  if file then
    local content = file:read("*all")
    file:close()
    luaunit.assertStrContains(
      content,
      "minkowski",
      "Minkowski command not found in generated SCAD file"
    )
  end
end

-- Run the tests
if not ... then
  os.exit(luaunit.LuaUnit.run())
end
