local luaunit = require("luaunit")
require("lib_geometry")

TestGeometry = {}

function TestGeometry:setUp()
  -- No special setup needed
end

function TestGeometry:testGetTriangleFromSides()
  -- Test equilateral triangle (all sides equal)
  local side = 10
  local pointA, pointB, pointC, angle =
    geometry.gettrianglefromsides(side, side, side)

  -- Check point coordinates
  luaunit.assertEquals(pointA, { 0, 0 }, "Point A should be at origin")
  luaunit.assertEquals(pointB, { side, 0 }, "Point B should be at (side, 0)")

  -- For equilateral triangle, the height is side * sqrt(3)/2
  local expectedHeight = geometry.round(side * math.sqrt(3) / 2)
  luaunit.assertEquals(
    pointC[2],
    expectedHeight,
    "Point C height should be correct"
  )

  -- For equilateral triangle, all angles are 60 degrees
  luaunit.assertAlmostEquals(angle, 60, 0.001, "Angle should be 60 degrees")

  -- Test right triangle (3-4-5 triangle)
  local a, b, c = 3, 4, 5
  pointA, pointB, pointC, angle = geometry.gettrianglefromsides(a, b, c)

  luaunit.assertEquals(pointA, { 0, 0 }, "Point A should be at origin")
  luaunit.assertEquals(pointB, { c, 0 }, "Point B should be at (c, 0)")

  -- Check that the triangle has approximately the right shape
  -- The calculation uses round which can affect precision
  local expectedY = geometry.round(
    math.sin(math.acos(((b * b) + (c * c) - (a * a)) / (2 * b * c))) * b
  )
  luaunit.assertEquals(
    pointC[2],
    expectedY,
    "Point C height should match calculation"
  )

  -- The angle should match the calculation from the cosine law
  local expectedAngle =
    math.deg(math.acos(((b * b) + (c * c) - (a * a)) / (2 * b * c)))
  luaunit.assertAlmostEquals(
    angle,
    expectedAngle,
    0.001,
    "Angle should match the cosine law"
  )

  -- Test scalene triangle
  a, b, c = 7, 8, 9
  pointA, pointB, pointC, angle = geometry.gettrianglefromsides(a, b, c)

  luaunit.assertEquals(pointA, { 0, 0 }, "Point A should be at origin")
  luaunit.assertEquals(pointB, { c, 0 }, "Point B should be at (c, 0)")

  -- Verify using law of cosines
  -- cos(alpha) = (b^2 + c^2 - a^2) / (2*b*c)
  local cosAlpha = ((b * b) + (c * c) - (a * a)) / (2 * b * c)
  local expectedAngle = math.deg(math.acos(cosAlpha))
  luaunit.assertAlmostEquals(
    angle,
    expectedAngle,
    0.001,
    "Angle should match law of cosines"
  )
end

-- Run the tests
if not ... then
  os.exit(luaunit.LuaUnit.run())
end
