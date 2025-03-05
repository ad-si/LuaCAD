local luaunit = require("luaunit")
require("lib_cad")

TestSimple = {}

function TestSimple:setUp()
  -- Create temp directory if it doesn't exist
  os.execute("mkdir -p temp")
end

function TestSimple:testCubeCreation()
  local cube = cad.cube { size = { 1, 2, 3 } }
  cube:export("temp/test_simple_cube.scad")

  -- Verify file was created
  local file = io.open("temp/test_simple_cube.scad", "r")
  luaunit.assertNotNil(file, "SCAD file for cube was not created")
  if file then
    file:close()
  end
end

function TestSimple:testSphereCreation()
  local sphere = cad.sphere { r = 2 }
  sphere:export("temp/test_simple_sphere.scad")

  -- Verify file was created
  local file = io.open("temp/test_simple_sphere.scad", "r")
  luaunit.assertNotNil(file, "SCAD file for sphere was not created")
  if file then
    file:close()
  end
end

function TestSimple:testCylinderCreation()
  -- Test with radius
  local cylinder1 = cad.cylinder { h = 10, r = 5 }
  cylinder1:export("temp/test_simple_cylinder_r.scad")

  -- Test with diameter
  local cylinder2 = cad.cylinder { h = 10, d = 10 }
  cylinder2:export("temp/test_simple_cylinder_d.scad")

  -- Test cone with r1/r2
  local cylinder3 = cad.cylinder { h = 10, r1 = 5, r2 = 2 }
  cylinder3:export("temp/test_simple_cylinder_cone_r.scad")

  -- Test cone with d1/d2
  local cylinder4 = cad.cylinder { h = 10, d1 = 10, d2 = 4 }
  cylinder4:export("temp/test_simple_cylinder_cone_d.scad")

  -- Test centered cylinder
  local cylinder5 = cad.cylinder { h = 10, r = 5, center = true }
  cylinder5:export("temp/test_simple_cylinder_centered.scad")

  -- Verify files were created
  local files = {
    "temp/test_simple_cylinder_r.scad",
    "temp/test_simple_cylinder_d.scad",
    "temp/test_simple_cylinder_cone_r.scad",
    "temp/test_simple_cylinder_cone_d.scad",
    "temp/test_simple_cylinder_centered.scad",
  }

  for _, filepath in ipairs(files) do
    local file = io.open(filepath, "r")
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
  local file = io.open("temp/test_simple_combined.scad", "r")
  luaunit.assertNotNil(file, "SCAD file for combined shapes was not created")
  if file then
    file:close()
  end
end

function TestSimple:testShowOnlyModifier()
  -- Create objects
  local cube1 = cad.cube { size = { 10, 10, 10 } }
  local cube2 =
    cad.cube({ size = { 5, 5, 5 } }):translate { x = 20, y = 0, z = 0 }
  local sphere1 = cad.sphere({ r = 7 }):translate { x = 0, y = 20, z = 0 }

  -- Test normal export (should export all objects)
  local testFile1 = "temp/test_show_only_all.scad"
  cad.export(testFile1, cube1, cube2, sphere1)

  -- Verify file was created
  local file1 = io.open(testFile1, "r")
  luaunit.assertNotNil(file1, "SCAD file for all objects was not created")
  if file1 then
    file1:close()
  end

  -- Test with one showOnly object (should only export the sphere)
  local testFile2 = "temp/test_show_only_sphere.scad"
  cad.export(testFile2, cube1, cube2, cad.s(sphere1))

  -- Verify file was created
  local file2 = io.open(testFile2, "r")
  luaunit.assertNotNil(file2, "SCAD file for show-only sphere was not created")
  if file2 then
    file2:close()
  end

  -- Test with multiple showOnly objects
  local testFile3 = "temp/test_show_only_multiple.scad"
  cad.export(testFile3, cube1, cad.s(cube2), cad.s(sphere1))

  -- Verify file was created
  local file3 = io.open(testFile3, "r")
  luaunit.assertNotNil(
    file3,
    "SCAD file for multiple show-only objects was not created"
  )
  if file3 then
    file3:close()
  end
end

-- Run the tests
if not ... then
  os.exit(luaunit.LuaUnit.run())
end
