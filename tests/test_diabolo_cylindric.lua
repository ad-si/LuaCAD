-- Test creating a diabolo shape with cylinders at the ends
local luaunit = require("luaunit")
require("luacad")

TestDiaboloCylindric = {}

function TestDiaboloCylindric:setUp()
  -- Default parameters
  self.height = 60 / 2
  self.radius_bottom = 50 / 2
  self.radius_middle = 20 / 2
  self.thickness = 10 / 2
  self.frags = 6

  -- Create temp directory if it doesn't exist
  os.execute("mkdir -p tests/temp")
end

function TestDiaboloCylindric:testDiaboloCreation()
  -- Calculate angle
  local h = self.height
  local alpha = math.atan((h / 2) / (self.radius_bottom - self.radius_middle))
  local alpha_deg = tonumber(string.format("%.2f", math.deg(alpha)))

  local cad_file = "temp/diabolo_cylindric_test.stl"

  -- Create the model with updated table parameter style
  local cylinder = (
    cad
      .cylinder({
        h = h / 2,
        r1 = self.radius_bottom,
        r2 = self.radius_middle,
      })
      :translate(0, 0, 0)
    - cad
      .cylinder({
        h = h / 2,
        r1 = self.radius_bottom - self.thickness,
        r2 = self.radius_middle - self.thickness,
      })
      :translate(0, 0, 0)
  )
    + (
      cad
        .cylinder({
          h = h / 2,
          r1 = self.radius_middle,
          r2 = self.radius_bottom,
        })
        :translate(0, 0, h / 2)
      - cad
        .cylinder({
          h = h / 2,
          r1 = self.radius_middle - self.thickness,
          r2 = self.radius_bottom - self.thickness,
        })
        :translate(0, 0, h / 2)
    )

  cylinder:export(cad_file)

  -- Verify file was created
  local file = io.open("tests/" .. cad_file, "r")
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

  -- Create the model with custom parameters and table parameter style
  local cylinder = (
    cylinder({ h = h / 2, r1 = rb, r2 = rm }):translate(0, 0, 0)
    - cylinder({ h = h / 2, r1 = rb - t, r2 = rm - t }):translate(0, 0, 0)
  )
    + (
      cylinder({ h = h / 2, r1 = rm, r2 = rb }):translate(0, 0, h / 2)
      - cad
        .cylinder({ h = h / 2, r1 = rm - t, r2 = rb - t })
        :translate(0, 0, h / 2)
    )

  cylinder:export(cad_file)

  -- Verify file was created
  local file = io.open("tests/" .. cad_file, "r")
  luaunit.assertNotNil(file, "STL file with custom parameters was not created")
  if file then
    file:close()
  end
end

-- Run the tests
if not ... then
  os.exit(luaunit.LuaUnit.run())
end
