local luaunit = require("luaunit")
require("lib_dxf")

TestDxfPolygon = {}

function TestDxfPolygon:setUp()
  -- Create temp directory if it doesn't exist
  os.execute("mkdir -p temp")
  self.dist = 20
end

function TestDxfPolygon:testBasicPolygon()
  local dxf_obj = dxf()

  local points = {
    { 0, 0 },
    { self.dist, self.dist },
    { 2 * self.dist, 0 },
    { 3 * self.dist, self.dist },
    { 4 * self.dist, 0 },
    { 5 * self.dist, self.dist },
    { 6 * self.dist, 0 },
    { 6 * self.dist, 3 * self.dist },
    { 4 * self.dist, 3 * self.dist },
    { 4 * self.dist, 2 * self.dist },
    { 3 * self.dist, 2 * self.dist },
    { 3 * self.dist, 3 * self.dist },
    { 2 * self.dist, 3 * self.dist },
    { 2 * self.dist, 2 * self.dist },
    { 0, 2 * self.dist },
    { 0, 0 },
  }

  dxf_obj:polygon(0, 0, points)
  dxf_obj:save("temp/dxf_test_basic_polygon.dxf")

  -- Verify file was created
  local file = io.open("temp/dxf_test_basic_polygon.dxf", "r")
  luaunit.assertNotNil(file, "DXF file for basic polygon was not created")
  if file then
    file:close()
  end
end

function TestDxfPolygon:testPolygonWithSpecialPoints()
  local dxf_obj = dxf()

  local points = {
    { 0, 0 },
    { self.dist, self.dist, "radius", { -5 } },
    { 2 * self.dist, 0, "radius", { 3 } },
    { 3 * self.dist, self.dist },
    { 4 * self.dist, 0 },
    { 5 * self.dist, self.dist },
    { 6 * self.dist, 0, "belly", { 10 } },
    { 6 * self.dist, 3 * self.dist },
    { 4 * self.dist, 3 * self.dist, "belly", { -4 } },
    { 4 * self.dist, 2 * self.dist },
    { 3 * self.dist, 2 * self.dist },
    { 3 * self.dist, 3 * self.dist, "arc", { -4 } },
    { 2 * self.dist, 3 * self.dist, "arc", { 4 } },
    { 2 * self.dist, 2 * self.dist, "radius", { -5 } },
    { 0, 2 * self.dist },
    { 0, 0 },
  }

  dxf_obj:polygon(0, 0, points)
  dxf_obj:save("temp/dxf_test_special_polygon.dxf")

  -- Verify file was created
  local file = io.open("temp/dxf_test_special_polygon.dxf", "r")
  luaunit.assertNotNil(
    file,
    "DXF file for polygon with special points was not created"
  )
  if file then
    file:close()
  end
end

function TestDxfPolygon:testMultiplePolygons()
  local dxf_obj = dxf()

  -- First polygon
  local points1 = {
    { 0, 0 },
    { self.dist, self.dist },
    { 2 * self.dist, 0 },
    { 0, 0 },
  }

  -- Second polygon
  local points2 = {
    { 0, 0 },
    { self.dist, self.dist },
    { 0, 2 * self.dist },
    { 0, 0 },
  }

  dxf_obj:polygon(0, 0, points1)
  dxf_obj:polygon(3 * self.dist, 0, points2)
  dxf_obj:save("temp/dxf_test_multiple_polygons.dxf")

  -- Verify file was created
  local file = io.open("temp/dxf_test_multiple_polygons.dxf", "r")
  luaunit.assertNotNil(file, "DXF file for multiple polygons was not created")
  if file then
    file:close()
  end
end

-- Run the tests
if not ... then
  os.exit(luaunit.LuaUnit.run())
end
