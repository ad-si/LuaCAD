--[[============================================================================
  Test file for testing error handling in variable creation
==============================================================================]]

local luaunit = require("luaunit")
require("luacad")

TestVarErrors = {}

function TestVarErrors:setUp()
  -- Create temp directory if it doesn't exist
  os.execute("mkdir -p tests/temp")
end

function TestVarErrors:testInvalidVariableName()
  -- Test that an error is raised for invalid variable name
  luaunit.assertErrorMsgContains("Invalid variable name", function()
    local a = var("1invalid", 10)
  end)
end

function TestVarErrors:testNonStringVariableName()
  -- Test that an error is raised for non-string variable name
  luaunit.assertErrorMsgContains("must be a string", function()
    local b = var(123, 10)
  end)
end

function TestVarErrors:testValidVariableUsage()
  -- Test that valid variables can be created and used in a model
  local valid_var = var("valid_var", 5)
  local model = cube { size = { valid_var, valid_var, valid_var } }
  model:export("temp/test_var_valid.scad")

  -- Verify file was created
  local file = io.open("tests/temp/test_var_valid.scad", "r")
  luaunit.assertNotNil(file, "SCAD file was not created")
  if file then
    local content = file:read("*all")
    file:close()
    luaunit.assertStrContains(
      content,
      "valid_var",
      false,
      "Variable not found in generated SCAD file"
    )
  end
end

-- Run the tests
if not ... then
  os.exit(luaunit.LuaUnit.run())
end
