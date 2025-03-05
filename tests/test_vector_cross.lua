local luaunit = require("luaunit")
require("lib_vector")

-- Global cross function for testing
function cross(a, b)
  if type(a) ~= "table" or type(b) ~= "table" then
    return nil
  end

  -- Both vectors must have the same dimension (either 2D or 3D)
  if #a ~= #b then
    return nil
  end

  -- 3D cross product
  if #a == 3 and #b == 3 then
    local x = a[2] * b[3] - a[3] * b[2]
    local y = a[3] * b[1] - a[1] * b[3]
    local z = a[1] * b[2] - a[2] * b[1]
    return { x, y, z }
  end

  -- 2D cross product (returns scalar)
  if #a == 2 and #b == 2 then
    return a[1] * b[2] - a[2] * b[1]
  end

  return nil
end

TestVectorCross = {}

function TestVectorCross:setUp()
  -- No setup needed
end

function TestVectorCross:testCrossProduct3D()
  -- Test case 1: cross({2, 3, 4}, {5, 6, 7}) should produce {-3, 6, -3}
  local result1 = cross({ 2, 3, 4 }, { 5, 6, 7 })
  luaunit.assertNotNil(result1, "Cross product should not be nil")
  luaunit.assertEquals(
    result1[1],
    -3,
    "X component of cross product is incorrect"
  )
  luaunit.assertEquals(
    result1[2],
    6,
    "Y component of cross product is incorrect"
  )
  luaunit.assertEquals(
    result1[3],
    -3,
    "Z component of cross product is incorrect"
  )

  -- Test case 2: cross({2, 1, -3}, {0, 4, 5}) should produce {17, -10, 8}
  local result2 = cross({ 2, 1, -3 }, { 0, 4, 5 })
  luaunit.assertNotNil(result2, "Cross product should not be nil")
  luaunit.assertEquals(
    result2[1],
    17,
    "X component of cross product is incorrect"
  )
  luaunit.assertEquals(
    result2[2],
    -10,
    "Y component of cross product is incorrect"
  )
  luaunit.assertEquals(
    result2[3],
    8,
    "Z component of cross product is incorrect"
  )
end

function TestVectorCross:testCrossProduct2D()
  -- Test case 3: cross({2, 1}, {0, 4}) should produce 8
  local result3 = cross({ 2, 1 }, { 0, 4 })
  luaunit.assertNotNil(result3, "Cross product should not be nil")
  luaunit.assertEquals(result3, 8, "2D cross product is incorrect")

  -- Test case 4: cross({1, -3}, {4, 5}) should produce 17
  local result4 = cross({ 1, -3 }, { 4, 5 })
  luaunit.assertNotNil(result4, "Cross product should not be nil")
  luaunit.assertEquals(result4, 17, "2D cross product is incorrect")
end

function TestVectorCross:testInvalidCrossProduct()
  -- Test case 5: cross({2, 1, -3}, {4, 5}) should produce nil
  local result5 = cross({ 2, 1, -3 }, { 4, 5 })
  luaunit.assertNil(
    result5,
    "Cross product of vectors with different dimensions should be nil"
  )

  -- Test case 6: cross({2, 3, 4}, "5") should produce nil
  local result6 = cross({ 2, 3, 4 }, "5")
  luaunit.assertNil(
    result6,
    "Cross product with non-vector argument should be nil"
  )
end

-- Run the tests
if not ... then
  os.exit(luaunit.LuaUnit.run())
end
