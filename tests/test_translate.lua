local luaunit = require("luaunit")
require("luascad")

TestTranslate = {}

function TestTranslate:setUp()
  -- Create temp directory if it doesn't exist
  os.execute("mkdir -p tests/temp")
end

function TestTranslate:testDirectParameters()
  local cube = cube { size = { 1, 1, 1 } }
  local translated = cube:translate(10, 20, 30)
  translated:export("temp/test_translate_direct.scad")

  -- Verify file was created
  local file = io.open("tests/temp/test_translate_direct.scad", "r")
  luaunit.assertNotNil(file, "SCAD file was not created")
  if file then
    local content = file:read("*all")
    file:close()
    -- Check that the translation parameter values are in the file
    luaunit.assertTrue(
      string.find(content, "translate") ~= nil,
      "translate function not found"
    )
    luaunit.assertTrue(string.find(content, "10") ~= nil, "x value not found")
    luaunit.assertTrue(string.find(content, "20") ~= nil, "y value not found")
    luaunit.assertTrue(string.find(content, "30") ~= nil, "z value not found")
  end
end

function TestTranslate:testObjectParameter()
  local cube = cube { size = { 1, 1, 1 } }
  local translated = cube:translate { x = 10, y = 20, z = 30 }
  translated:export("temp/test_translate_object.scad")

  -- Verify file was created
  local file = io.open("tests/temp/test_translate_object.scad", "r")
  luaunit.assertNotNil(file, "SCAD file was not created")
  if file then
    local content = file:read("*all")
    file:close()
    -- Check that the translation parameter values are in the file
    luaunit.assertTrue(
      string.find(content, "translate") ~= nil,
      "translate function not found"
    )
    luaunit.assertTrue(string.find(content, "10") ~= nil, "x value not found")
    luaunit.assertTrue(string.find(content, "20") ~= nil, "y value not found")
    luaunit.assertTrue(string.find(content, "30") ~= nil, "z value not found")
  end
end

function TestTranslate:testArrayParameter()
  local cube = cube { size = { 1, 1, 1 } }
  local translated = cube:translate { 10, 20, 30 }
  translated:export("temp/test_translate_array.scad")

  -- Verify file was created
  local file = io.open("tests/temp/test_translate_array.scad", "r")
  luaunit.assertNotNil(file, "SCAD file was not created")
  if file then
    local content = file:read("*all")
    file:close()
    -- Check that the translation parameter values are in the file
    luaunit.assertTrue(
      string.find(content, "translate") ~= nil,
      "translate function not found"
    )
    luaunit.assertTrue(string.find(content, "10") ~= nil, "x value not found")
    luaunit.assertTrue(string.find(content, "20") ~= nil, "y value not found")
    luaunit.assertTrue(string.find(content, "30") ~= nil, "z value not found")
  end
end

function TestTranslate:testVectorParameter()
  local cube = cube { size = { 1, 1, 1 } }
  local translated = cube:translate { v = { 10, 20, 30 } }
  translated:export("temp/test_translate_vector.scad")

  -- Verify file was created
  local file = io.open("tests/temp/test_translate_vector.scad", "r")
  luaunit.assertNotNil(file, "SCAD file was not created")
  if file then
    local content = file:read("*all")
    file:close()
    -- Check that the translation parameter values are in the file
    luaunit.assertTrue(
      string.find(content, "translate") ~= nil,
      "translate function not found"
    )
    luaunit.assertTrue(string.find(content, "10") ~= nil, "x value not found")
    luaunit.assertTrue(string.find(content, "20") ~= nil, "y value not found")
    luaunit.assertTrue(string.find(content, "30") ~= nil, "z value not found")
  end
end

-- Run the tests
if not ... then
  os.exit(luaunit.LuaUnit.run())
end
