-- Test creating a diabolo shape with cylinders at the ends
local luaunit = require("luaunit")
require("lib_cad")

TestDiaboloCylindric = {}

function TestDiaboloCylindric:setUp()
  -- Default parameters
  self.height = 60 / 2
  self.radius_bottom = 50 / 2
  self.radius_middle = 20 / 2
  self.thickness = 10 / 2
  self.frags = 6

  -- Create temp directory if it doesn't exist
  os.execute("mkdir -p temp")
end

function TestDiaboloCylindric:testDiaboloCreation()
  -- Calculate angle
  local h = self.height
  local alpha = math.atan((h / 2) / (self.radius_bottom - self.radius_middle))
  local alpha_deg = tonumber(string.format("%.2f", math.deg(alpha)))

  local cad_file = "temp/diabolo_cylindric_test.stl"

  -- Create the model
  local cylinder = (
    cad.cylinder(0, 0, 0, h / 2, self.radius_bottom, self.radius_middle)
    - cad.cylinder(
      0,
      0,
      0,
      h / 2,
      self.radius_bottom - self.thickness,
      self.radius_middle - self.thickness
    )
  )
    + (
      cad.cylinder(0, 0, h / 2, h / 2, self.radius_middle, self.radius_bottom)
      - cad.cylinder(
        0,
        0,
        h / 2,
        h / 2,
        self.radius_middle - self.thickness,
        self.radius_bottom - self.thickness
      )
    )

  cylinder:export(cad_file)

  -- Verify file was created
  local file = io.open(cad_file, "r")
  luaunit.assertNotNil(file, "STL file was not created")
  if file then
    file:close()
  end
end

function TestDiaboloCylindric:testCustomParameters()
  -- Custom parameters
  local h = 40
  local rb = 30
  local rm = 15
  local t = 5

  local cad_file = "temp/diabolo_cylindric_custom_test.stl"

  -- Create the model with custom parameters
  local cylinder = (
    cad.cylinder(0, 0, 0, h / 2, rb, rm)
    - cad.cylinder(0, 0, 0, h / 2, rb - t, rm - t)
  )
    + (
      cad.cylinder(0, 0, h / 2, h / 2, rm, rb)
      - cad.cylinder(0, 0, h / 2, h / 2, rm - t, rb - t)
    )

  cylinder:export(cad_file)

  -- Verify file was created
  local file = io.open(cad_file, "r")
  luaunit.assertNotNil(file, "STL file with custom parameters was not created")
  if file then
    file:close()
  end
end

-- Run the tests
if not ... then
  os.exit(luaunit.LuaUnit.run())
end
