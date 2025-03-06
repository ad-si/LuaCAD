local luaunit = require("luaunit")
require("lib_cad")

TestSphere = {}

function TestSphere:setUp()
  -- Create temp directory if it doesn't exist
  os.execute("mkdir -p tests/temp")
end

function TestSphere:testSphereCreationWithRadius()
  local sphere = cad.sphere { r = 5 }
  sphere:export("temp/test_sphere_radius.scad")

  -- Verify file was created
  local file = io.open("tests/temp/test_sphere_radius.scad", "r")
  luaunit.assertNotNil(file, "SCAD file for sphere with radius was not created")
  if file then
    local content = file:read("*all")
    file:close()
    luaunit.assertStrContains(
      content,
      "sphere",
      "Sphere command not found in generated SCAD file"
    )
    luaunit.assertStrContains(
      content,
      "r = 5",
      "Radius parameter not found in generated SCAD file"
    )
  end
end

function TestSphere:testSphereCreationWithDiameter()
  local sphere = cad.sphere { d = 10 }
  sphere:export("temp/test_sphere_diameter.scad")

  -- Verify file was created
  local file = io.open("tests/temp/test_sphere_diameter.scad", "r")
  luaunit.assertNotNil(
    file,
    "SCAD file for sphere with diameter was not created"
  )
  if file then
    local content = file:read("*all")
    file:close()
    luaunit.assertStrContains(
      content,
      "sphere",
      "Sphere command not found in generated SCAD file"
    )
    luaunit.assertStrContains(
      content,
      "d = 10",
      "Diameter parameter not found in generated SCAD file"
    )
  end
end

function TestSphere:testSphereTranslation()
  local sphere = cad.sphere { r = 3 }
  sphere:translate(10, 10, 10)
  sphere:export("temp/test_sphere_translated.scad")

  -- Verify file was created
  local file = io.open("tests/temp/test_sphere_translated.scad", "r")
  luaunit.assertNotNil(file, "SCAD file for translated sphere was not created")
  if file then
    local content = file:read("*all")
    file:close()
    luaunit.assertStrContains(
      content,
      "sphere",
      "Sphere command not found in generated SCAD file"
    )
    luaunit.assertStrContains(
      content,
      "translate",
      "Translate command not found in generated SCAD file"
    )
    luaunit.assertStrContains(
      content,
      "[10,10,10]",
      "Translate coordinates not found in generated SCAD file"
    )
  end
end

function TestSphere:testSphereColorSetting()
  local sphere = cad.sphere { r = 4 }
  sphere:setcolor("red")
  sphere:export("temp/test_sphere_colored.scad")

  -- Verify file was created
  local file = io.open("tests/temp/test_sphere_colored.scad", "r")
  luaunit.assertNotNil(file, "SCAD file for colored sphere was not created")
  if file then
    local content = file:read("*all")
    file:close()
    luaunit.assertStrContains(
      content,
      "sphere",
      "Sphere command not found in generated SCAD file"
    )
  end
end

function TestSphere:testMissingParameters()
  -- Should raise an error when neither radius nor diameter is provided
  luaunit.assertErrorMsgContains("No radius or diameter was given", function()
    cad.sphere {}
  end)
end

-- Run the tests
if not ... then
  os.exit(luaunit.LuaUnit.run())
end
