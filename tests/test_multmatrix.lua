local luaunit = require("luaunit")
require("lib_cad")

TestMultMatrix = {}

function TestMultMatrix:setUp()
  -- Create temp directory if it doesn't exist
  os.execute("mkdir -p tests/temp")
end

function TestMultMatrix:testBasicMultMatrix()
  -- Create a simple cube
  local cube = cad.cube { size = { 2, 2, 2 } }

  -- Identity matrix - should not change the cube
  local identity_matrix = {
    1,
    0,
    0,
    0, -- First row
    0,
    1,
    0,
    0, -- Second row
    0,
    0,
    1,
    0, -- Third row
    0,
    0,
    0,
    1, -- Fourth row
  }

  -- Apply the identity matrix
  local transformed = cube:multmatrix(identity_matrix)
  transformed:export("temp/test_multmatrix_identity.scad")

  -- Verify file was created
  local file = io.open("tests/temp/test_multmatrix_identity.scad", "r")
  luaunit.assertNotNil(
    file,
    "SCAD file for identity multmatrix was not created"
  )
  if file then
    local content = file:read("*all")
    file:close()
    luaunit.assertStrContains(
      content,
      "multmatrix",
      "Multmatrix command not found in generated SCAD file"
    )
    luaunit.assertStrContains(
      content,
      "[1,0,0,0]",
      "Identity matrix not found in generated SCAD file"
    )
  end
end

function TestMultMatrix:testScaleWithMultMatrix()
  -- Create a simple cube
  local cube = cad.cube { size = { 1, 1, 1 } }

  -- Scale matrix - scale 2x in all dimensions
  local scale_matrix = {
    2,
    0,
    0,
    0, -- First row
    0,
    2,
    0,
    0, -- Second row
    0,
    0,
    2,
    0, -- Third row
    0,
    0,
    0,
    1, -- Fourth row
  }

  -- Apply the scale matrix
  local transformed = cube:multmatrix(scale_matrix)
  transformed:export("temp/test_multmatrix_scale.scad")

  -- Verify file was created
  local file = io.open("tests/temp/test_multmatrix_scale.scad", "r")
  luaunit.assertNotNil(file, "SCAD file for scale multmatrix was not created")
  if file then
    local content = file:read("*all")
    file:close()
    luaunit.assertStrContains(
      content,
      "multmatrix",
      "Multmatrix command not found in generated SCAD file"
    )
    luaunit.assertStrContains(
      content,
      "[2,0,0,0]",
      "Scale matrix not found in generated SCAD file"
    )
  end
end

function TestMultMatrix:testTranslationWithMultMatrix()
  -- Create a simple cube
  local cube = cad.cube { size = { 1, 1, 1 } }

  -- Translation matrix - move 3 units in x, 2 in y, 1 in z
  local translation_matrix = {
    1,
    0,
    0,
    3, -- First row
    0,
    1,
    0,
    2, -- Second row
    0,
    0,
    1,
    1, -- Third row
    0,
    0,
    0,
    1, -- Fourth row
  }

  -- Apply the translation matrix
  local transformed = cube:multmatrix(translation_matrix)
  transformed:export("temp/test_multmatrix_translation.scad")

  -- Verify file was created
  local file = io.open("tests/temp/test_multmatrix_translation.scad", "r")
  luaunit.assertNotNil(
    file,
    "SCAD file for translation multmatrix was not created"
  )
  if file then
    local content = file:read("*all")
    file:close()
    luaunit.assertStrContains(
      content,
      "multmatrix",
      "Multmatrix command not found in generated SCAD file"
    )
    luaunit.assertStrContains(
      content,
      "[1,0,0,3]",
      "Translation matrix not found in generated SCAD file"
    )
  end
end

function TestMultMatrix:testRotationWithMultMatrix()
  -- Create a simple cube
  local cube = cad.cube { size = { 1, 1, 1 } }

  -- Rotation matrix - rotate 45 degrees around Z axis
  -- sin(45°) = cos(45°) = 0.7071067811865475
  local sin45 = 0.7071067811865475
  local cos45 = 0.7071067811865475

  local rotation_matrix = {
    cos45,
    -sin45,
    0,
    0, -- First row
    sin45,
    cos45,
    0,
    0, -- Second row
    0,
    0,
    1,
    0, -- Third row
    0,
    0,
    0,
    1, -- Fourth row
  }

  -- Apply the rotation matrix
  local transformed = cube:multmatrix(rotation_matrix)
  transformed:export("temp/test_multmatrix_rotation.scad")

  -- Verify file was created
  local file = io.open("tests/temp/test_multmatrix_rotation.scad", "r")
  luaunit.assertNotNil(
    file,
    "SCAD file for rotation multmatrix was not created"
  )
  if file then
    local content = file:read("*all")
    file:close()
    luaunit.assertStrContains(
      content,
      "multmatrix",
      "Multmatrix command not found in generated SCAD file"
    )
    -- Check for rotation matrix values (approximately)
    luaunit.assertStrContains(
      content,
      "0.70710678118655",
      "Rotation matrix values not found in generated SCAD file"
    )
  end
end

function TestMultMatrix:testCombinationWithMultMatrix()
  -- Create a simple cube
  local cube = cad.cube { size = { 1, 1, 1 } }

  -- Combined matrix - scale, rotate, and translate
  -- First scale by 2, then rotate 45° around Z, then translate by (3,2,1)
  local sin45 = 0.7071067811865475
  local cos45 = 0.7071067811865475

  local combined_matrix = {
    2 * cos45,
    -2 * sin45,
    0,
    3, -- First row
    2 * sin45,
    2 * cos45,
    0,
    2, -- Second row
    0,
    0,
    2,
    1, -- Third row
    0,
    0,
    0,
    1, -- Fourth row
  }

  -- Apply the combined matrix
  local transformed = cube:multmatrix(combined_matrix)
  transformed:export("temp/test_multmatrix_combined.scad")

  -- Verify file was created
  local file = io.open("tests/temp/test_multmatrix_combined.scad", "r")
  luaunit.assertNotNil(
    file,
    "SCAD file for combined multmatrix was not created"
  )
  if file then
    local content = file:read("*all")
    file:close()
    luaunit.assertStrContains(
      content,
      "multmatrix",
      "Multmatrix command not found in generated SCAD file"
    )
  end
end

function TestMultMatrix:testInvalidMatrixSize()
  -- Create a simple cube
  local cube = cad.cube { size = { 1, 1, 1 } }

  -- Invalid matrix with only 9 elements
  local invalid_matrix = {
    1,
    0,
    0,
    0,
    1,
    0,
    0,
    0,
    1,
  }

  -- Attempt to apply the invalid matrix, should throw an error
  luaunit.assertErrorMsgContains(
    "multmatrix requires a 4x4 matrix (16 elements)",
    function()
      cube:multmatrix(invalid_matrix)
    end
  )
end

-- Run the tests
if not ... then
  os.exit(luaunit.LuaUnit.run())
end
