local luaunit = require("luaunit")
require("lib_cad")

TestRotateExtrude = {}

-- Helper function to check if a pattern exists in the content using pattern matching
local function patternExists(content, pattern)
  -- Escape pattern special characters to create a Lua pattern that matches literal text
  local luaPattern = pattern:gsub("([%(%)%.%%%+%-%*%?%[%]%^%$])", "%%%1")
  return content:match(luaPattern) ~= nil
end

function TestRotateExtrude:setUp()
  -- Create temp directory if it doesn't exist
  os.execute("mkdir -p temp")
end

function TestRotateExtrude:testSimpleRotateExtrude()
  -- Create a circle that will be rotated to form a torus
  local circle = cad.circle({ r = 2 }):translate(5, 0, 0)
  local torus = circle:rotateextrude()

  torus:export("temp/test_rotate_extrude_simple.scad")

  -- Verify file was created
  local file = io.open("temp/test_rotate_extrude_simple.scad", "r")
  luaunit.assertNotNil(
    file,
    "SCAD file for simple rotate_extrude was not created"
  )
  if file then
    local content = file:read("*all")
    file:close()
    luaunit.assertTrue(
      patternExists(content, "rotate_extrude()"),
      "rotate_extrude() command not found in output file"
    )
  end
end

function TestRotateExtrude:testRotateExtrudeWithAngle()
  -- Create a circle that will be rotated only 180 degrees
  local circle = cad.circle({ r = 2 }):translate(5, 0, 0)
  local partial_torus = circle:rotateextrude(180)

  partial_torus:export("temp/test_rotate_extrude_angle.scad")

  -- Verify file was created
  local file = io.open("temp/test_rotate_extrude_angle.scad", "r")
  luaunit.assertNotNil(
    file,
    "SCAD file for rotate_extrude with angle was not created"
  )
  if file then
    local content = file:read("*all")
    file:close()
    luaunit.assertTrue(
      patternExists(content, "rotate_extrude(angle = 180)"),
      "rotate_extrude(angle = 180) command not found in output file"
    )
  end
end

function TestRotateExtrude:testRotateExtrudeWithConvexity()
  -- Create a complex shape that needs convexity hint
  local polygon = cad
    .polygon({
      points = {
        { 1, 0 },
        { 1, 1 },
        { 2, 1 },
        { 2, 2 },
        { 1, 2 },
        { 1, 3 },
        { 0, 3 },
      },
    })
    :translate(3, 0, 0)

  local complex_solid = polygon:rotateextrude(nil, 10)

  complex_solid:export("temp/test_rotate_extrude_convexity.scad")

  -- Verify file was created
  local file = io.open("temp/test_rotate_extrude_convexity.scad", "r")
  luaunit.assertNotNil(
    file,
    "SCAD file for rotate_extrude with convexity was not created"
  )
  if file then
    local content = file:read("*all")
    file:close()
    luaunit.assertTrue(
      patternExists(content, "rotate_extrude(convexity = 10)"),
      "rotate_extrude(convexity = 10) command not found in output file"
    )
  end
end

function TestRotateExtrude:testRotateExtrudeWithAngleAndConvexity()
  -- Create a complex shape with both angle and convexity parameters
  local polygon = cad
    .polygon({
      points = {
        { 1, 0 },
        { 1, 1 },
        { 2, 1 },
        { 2, 2 },
        { 1, 2 },
        { 1, 3 },
        { 0, 3 },
      },
    })
    :translate(3, 0, 0)

  local complex_partial = polygon:rotateextrude(270, 10)

  complex_partial:export("temp/test_rotate_extrude_angle_convexity.scad")

  -- Verify file was created
  local file = io.open("temp/test_rotate_extrude_angle_convexity.scad", "r")
  luaunit.assertNotNil(
    file,
    "SCAD file for rotate_extrude with angle and convexity was not created"
  )
  if file then
    local content = file:read("*all")
    file:close()
    luaunit.assertTrue(
      patternExists(content, "rotate_extrude(angle = 270, convexity = 10)"),
      "rotate_extrude(angle = 270, convexity = 10) command not found in output file"
    )
  end
end

function TestRotateExtrude:testGlobalRotateExtrudeFunction()
  -- Test the global function cad.rotate_extrude
  local circle = cad.circle({ r = 2 }):translate(5, 0, 0)
  local torus = cad.rotate_extrude { angle = 180, convexity = 5 }

  -- Add the circle to the rotate_extrude operation
  torus:add(circle)

  torus:export("temp/test_rotate_extrude_global.scad")

  -- Verify file was created
  local file = io.open("temp/test_rotate_extrude_global.scad", "r")
  luaunit.assertNotNil(
    file,
    "SCAD file for global rotate_extrude function was not created"
  )
  if file then
    local content = file:read("*all")
    file:close()
    luaunit.assertTrue(
      patternExists(content, "rotate_extrude(angle = 180, convexity = 5)"),
      "rotate_extrude(angle = 180, convexity = 5) command not found in output file"
    )
  end
end

-- Run the tests
if not ... then
  os.exit(luaunit.LuaUnit.run())
end
