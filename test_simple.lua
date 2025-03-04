local luaunit = require('luaunit')
require "lib_cad"

TestSimple = {}

function TestSimple:setUp()
  -- Create temp directory if it doesn't exist
  os.execute("mkdir -p temp")
end

function TestSimple:testCubeCreation()
  local cube = cad.cube{size={1, 2, 3}}
  cube:export("temp/test_simple_cube.scad")
  
  -- Verify file was created
  local file = io.open("temp/test_simple_cube.scad", "r")
  luaunit.assertNotNil(file, "SCAD file for cube was not created")
  if file then file:close() end
end

function TestSimple:testSphereCreation()
  local sphere = cad.sphere(5, 0, 0, 2)
  sphere:export("temp/test_simple_sphere.scad")
  
  -- Verify file was created
  local file = io.open("temp/test_simple_sphere.scad", "r")
  luaunit.assertNotNil(file, "SCAD file for sphere was not created")
  if file then file:close() end
end

function TestSimple:testCombiningShapes()
  local cube = cad.cube{size={1, 2, 3}}
  local sphere = cad.sphere(5, 0, 0, 2)
  
  local model = cube + sphere
  model:export("temp/test_simple_combined.scad")
  
  -- Verify file was created
  local file = io.open("temp/test_simple_combined.scad", "r")
  luaunit.assertNotNil(file, "SCAD file for combined shapes was not created")
  if file then file:close() end
end

-- Run the tests
if not ... then
  os.exit(luaunit.LuaUnit.run())
end
