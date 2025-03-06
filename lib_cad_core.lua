--[[--================================================================================
  Lua CAD Core Functions
  Core functionality for the CAD library
--]]
--================================================================================

require("lib_geometry") -- includes lib_vector

--[[--------------------------------------------------------------------------------
  GLOBAL CAD TABLE
--]]
--------------------------------------------------------------------------------
cad = {}

-- OpenSCAD settings
cad.settings = require("lib_cad_settings")

--[[--------------------------------------------------------------------------------
  LOCAL OBJ HELPER FUNCTIONS
--]]
--------------------------------------------------------------------------------
local default_segments = cad.settings.default_segments
local executable = cad.settings.executable
local scadfile = cad.settings.scadfile

local function getExtension(path)
  return "." .. path:lower():match("%.([^%.]+)$")
end

local function intend_content(this, count)
  this.intend_num = this.intend_num + count
end

local function update_content(this, str)
  local s = string.rep("\t", this.intend_num)
  str = s
    .. string.gsub(str, "\n(.)", function(c)
      return "\n" .. s .. c
    end)
  this.scad_content = this.scad_content .. str
end

local function update_content_t(obj, scad_template, t)
  local replacementValue = (
    type(t) == "table"
    and t.tableType == "var"
    and t.value
  ) or t

  local _scad_content = string.gsub(scad_template, "%$(%w+)", function(key)
    if type(t[key]) == "table" then
      return t[key]
    end
    return t[key] or "ERROR: " .. key
  end)
  update_content(obj, _scad_content)
end

-- add aditonal presets of colors here
local color_rgb = {
  ["white"] = { 1.0, 1.0, 1.0 },
  ["black"] = { 0, 0, 0 },
  ["red"] = { 1.0, 0, 0 },
  ["green"] = { 0, 1.0, 0 },
  ["blue"] = { 0, 0, 1.0 },
  ["cyan"] = { 0, 1.0, 1.0 },
  ["magenta"] = { 1.0, 0, 1.0 },
  ["yellow"] = { 1.0, 1.0, 0 },
}

local function export_scad(obj, file)
  -- create native SCAD file
  local fscad = io.open(scadfile, "w")
  if not fscad then
    error("Failed to write to scad file: " .. scadfile)
  end
  fscad:write(cad.settings.include .. "\n")
  fscad:write("$fn = " .. obj.segments .. ";\n\n")

  -- Include variable assignments if they exist
  if cad.scad_variables and cad.scad_variables ~= "" then
    fscad:write(cad.scad_variables .. "\n")
  end

  fscad:write(obj.scad_content)
  fscad:close()

  if getExtension(file) == ".scad" then
    os.rename(scadfile, file)
  else
    -- Execute OpenSCAD to create the final output file
    os.execute(
      executable
        .. (" -o " .. file)
        .. (" " .. scadfile)
        .. (isTesting and " > /dev/null 2>&1" or "")
    )
  end
end

--[[--------------------------------------------------------------------------------
  GLOBAL CAD DIRECT FUNCTIONS, returning new obj
--]]
--------------------------------------------------------------------------------

--[[----------------------------------------
  function cad()

  returning new cad object
--]]
----------------------------------------
local cad_meta = {}
local cad_obj = function()
  local obj = setmetatable({}, cad_meta)
  obj:init()
  return obj
end
setmetatable(cad, { __call = cad_obj })

--[[----------------------------------------
  function cad.setdefaultcirclefragments(num)

  optional $fn and $fs can be set (http://en.wikibooks.org/wiki/OpenSCAD_User_Manual/Other_Language_Features#Special_variables)
--]]
----------------------------------------
cad.setdefaultcirclefragments = function(fn)
  default_segments = fn
end

--[[----------------------------------------
  function cad.sign(x)

  Returns the sign of a number:
   1 when x > 0
   0 when x = 0
  -1 when x < 0

  OpenSCAD compatibility function
--]]
----------------------------------------
cad.sign = function(x)
  return geometry.sign(x)
end

--[[--------------------------------------------------------------------------------
  GLOBAL OBJ FUNCTIONS via METATABLE
--]]
--------------------------------------------------------------------------------
cad_meta.__index = {}

--[[----------------------------------------
  function <cad>:init()

  resets the object
--]]
----------------------------------------
cad_meta.__index.init = function(obj)
  obj.scad_content = ""
  obj.segments = default_segments
  obj.intend_num = 0
  -- diffuse color [0,1]
  obj.color = { 1, 1, 1 }
  return obj
end
cad_meta.__index.reset = cad_meta.__index.init

--[[----------------------------------------
  function <cad>:copy()

  copies the object
--]]
----------------------------------------
cad_meta.__index.copy = function(obj)
  local copy = cad_obj()
  for i, v in pairs(obj) do
    copy[i] = v
  end
  return copy
end
cad_meta.__index.clone = cad_meta.__index.copy

--[[----------------------------------------
  function <cad>:setcirclefragments(num)

  optional $fn and $fs can be set (http://en.wikibooks.org/wiki/OpenSCAD_User_Manual/Other_Language_Features#Special_variables)
--]]
----------------------------------------
cad_meta.__index.setcirclefragments = function(obj, fn)
  obj.segments = fn
  return obj
end

--[[----------------------------------------
  function <cad>:setcolor(R, G, B)
  function <cad>:setcolor(color_name)
  function <cad>:setcolor({R, G, B})

  Set an object color, will work with obj and wrl format
  R,G,B are in the range of [0,1]
--]]
----------------------------------------
cad_meta.__index.setcolor = function(obj, R, G, B)
  if type(R) == "table" then
    obj.color = R
  elseif type(R) == "string" then
    if color_rgb[R] then
      obj.color = color_rgb[R]
    else
      error("Unable to set color: " .. R)
    end
  else
    obj.color = { R, G, B }
  end
  return obj
end

--[[----------------------------------------
  function <cad>:native(code)

  -- for executing native code
--]]
----------------------------------------
function cad_meta.__index.native(obj, str)
  update_content(obj, "\n//native\n" .. str .. "\n")
  return obj
end

--[[----------------------------------------
  function <cad>:import(file)

  import an stl/dfx here
--]]
----------------------------------------
function cad_meta.__index.import(obj, file)
  update_content(obj, "import(" .. string.format("%q", file) .. ");\n")
  return obj
end

cad_meta.__add = function(a, b)
  return a:add(b)
end

cad_meta.__sub = function(a, b)
  return a:sub(b)
end

-- Get the value of a variable or its variable name
function valOrName(var)
  if type(var) == "table" and var.tableType == "var" then
    return var.name or var.value
  end
  return var
end

function tableToStr(t)
  if type(t) ~= "table" then
    return "ERROR: " .. type(t) .. " " .. tostring(t)
  end
  local str = "TABLE_TO_STR {"
  for k, v in pairs(t) do
    str = str .. k .. "=" .. tableToStr(v) .. ","
  end
  return str .. "}"
end

-- Export helper functions for other modules
cad._helpers = {
  getExtension = getExtension,
  intend_content = intend_content,
  update_content = update_content,
  update_content_t = update_content_t,
  color_rgb = color_rgb,
  export_scad = export_scad,
  cad_obj = cad_obj,
  cad_meta = cad_meta,
  valOrName = valOrName,
  tableToStr = tableToStr,
}

-- Load other modules
require("lib_cad_2d")
require("lib_cad_3d")
require("lib_cad_transform")
require("lib_cad_export")
require("lib_cad_stl")
