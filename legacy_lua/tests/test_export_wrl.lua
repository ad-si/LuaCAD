--// create stl or dxf file

require("luacad")
local luaunit = require("luaunit")

-- output file
local cad_file = "test_export_wrl"

TestExportWrl = {}

function TestExportWrl:setUp()
  -- Create the test directory if it doesn't exist
  os.execute("mkdir -p tests/temp")
  self.cad_file = "temp/" .. cad_file
end

function TestExportWrl:testBoxExport()
  -- create cad object
  local box = cube { size = { 10, 20, 30 } }
  -- export box as STL first, since WRL requires STL conversion
  box:export(self.cad_file .. "_box.stl", { 0, 0, 1 })
  -- Verify file exists
  local file = io.open("tests/" .. self.cad_file .. "_box.stl")
  luaunit.assertNotNil(file, "STL file for box was not created")
  if file then
    file:close()
  end
end

function TestExportWrl:testCylinderExport()
  local cyl = cylinder { h = 50, r = 10 }
  -- export as stl
  cyl:export(self.cad_file .. "_cylinder.stl")
  -- Verify file exists
  local file = io.open("tests/" .. self.cad_file .. "_cylinder.stl")
  luaunit.assertNotNil(file, "STL file for cylinder was not created")
  if file then
    file:close()
  end
end

-- Run tests
if not ... then
  os.exit(luaunit.LuaUnit.run())
end
