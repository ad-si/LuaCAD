--[[--================================================================================
  LuaCAD Main Entry Point
  This is the main entry point for the LuaCAD library
--]]
--================================================================================

-- Add src directory to package path
local script_path = debug.getinfo(1, "S").source:sub(2):match("(.*/)") or "./"
if not script_path:match("/$") then
  script_path = script_path .. "/"
end
package.path = script_path .. "src/?.lua;" .. package.path

-- Load the core CAD module which will load all other modules
require("src.core.cad")

-- Export all cad functions to the global namespace
for name, func in pairs(cad) do
  if
    type(func) == "function"
    or (
      type(func) == "table"
      and getmetatable(func)
      and getmetatable(func).__call
    )
  then
    _G[name] = func
  end
end
