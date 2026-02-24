local luaunit = require("luaunit")
require("src.export.dxf")

TestDxfArc = {}

function TestDxfArc:setUp()
  -- Create temp directory if it doesn't exist
  os.execute("mkdir -p tests/temp")
end

function TestDxfArc:testBasicArc()
  local arc = dxf()
  arc:arc(10, 10, 50, 0, 45)
  arc:save("tests/temp/dxf_test_arc_basic.dxf")

  -- Verify file was created
  local file = io.open("tests/temp/dxf_test_arc_basic.dxf", "r")
  luaunit.assertNotNil(file, "DXF file for basic arc was not created")
  if file then
    file:close()
  end
end

function TestDxfArc:testNegativeStartAngle()
  local arc = dxf()
  arc:arc(100, 10, 30, -45, 90)
  arc:save("tests/temp/dxf_test_arc_negative.dxf")

  -- Verify file was created
  local file = io.open("tests/temp/dxf_test_arc_negative.dxf", "r")
  luaunit.assertNotNil(
    file,
    "DXF file for arc with negative start angle was not created"
  )
  if file then
    file:close()
  end
end

function TestDxfArc:testMultipleArcs()
  local arc = dxf()
  arc:arc(10, 10, 50, 0, 45)
  arc:arc(100, 10, 30, -45, 90)
  arc:save("tests/temp/dxf_test_arcs_multiple.dxf")

  -- Verify file was created
  local file = io.open("tests/temp/dxf_test_arcs_multiple.dxf", "r")
  luaunit.assertNotNil(file, "DXF file for multiple arcs was not created")
  if file then
    file:close()
  end
end

-- Run the tests
if not ... then
  os.exit(luaunit.LuaUnit.run())
end
