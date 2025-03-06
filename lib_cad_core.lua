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
      -- For tables, create a string representation
      local result = "["
      for i, v in ipairs(t[key]) do
        result = result .. (i > 1 and ", " or "") .. tostring(v)
      end
      return result .. "]"
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

-- Function to get the directory of the currently executing Lua script
local function get_script_path()
  -- Walk the call stack to find the original script
  local level = 1
  local main_info
  repeat
    local info = debug.getinfo(level, "S")
    if not info then
      break
    end

    -- Check if this is a main chunk (the top-level script file)
    if info.what == "main" and info.source:sub(1, 1) == "@" then
      main_info = info
      break
    end
    level = level + 1
  until level > 20 -- Limit the search depth

  -- If we found the main script, use it; otherwise fall back to caller
  local info = main_info or debug.getinfo(3, "S")

  local source = info.source
  -- Remove the @ prefix if present
  if source:sub(1, 1) == "@" then
    source = source:sub(2)
  end

  -- Determine the directory part of the script path
  local directory

  if source:match("^/") then
    -- It's an absolute path, get the directory part
    directory = source:match("(.*/)")
  else
    -- It's a relative path
    -- If it contains directory separators, extract the directory part
    if source:find("/") then
      directory = source:match("^(.-)[^/]+$")
    else
      -- It's just a filename with no path, use the current directory
      directory = "./"
    end
  end

  return directory or ""
end

local function export_scad(obj, file)
  -- If file is not an absolute path, make it relative to the script path
  if file:sub(1, 1) ~= "/" and file:sub(1, 1) ~= "\\" then
    local script_dir = get_script_path() or ""
    file = script_dir .. file
  end

  -- Debug output removed

  -- Get the directory part of the file path to ensure it exists
  local file_dir = file:match("(.*/)")
  if file_dir then
    -- Try to create the directory if it doesn't exist (will fail silently if it already exists)
    os.execute("mkdir -p " .. file_dir)
  end

  -- Create a temp file in the same directory as the output file
  local temp_scadfile
  if file:match("^/") then
    -- For absolute paths, put the temp file in the same directory
    temp_scadfile = file .. ".tmp.scad"
  else
    -- Otherwise use the global temp location
    temp_scadfile = scadfile
  end

  -- Create native SCAD file
  local fscad = io.open(temp_scadfile, "w")
  if not fscad then
    error("Failed to write to scad file: " .. temp_scadfile)
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
    os.rename(temp_scadfile, file)
  else
    -- Execute OpenSCAD to create the final output file
    os.execute(
      executable
        .. (" -o " .. file)
        .. (" " .. temp_scadfile)
        .. (isTesting and " > /dev/null 2>&1" or "")
    )
    -- Clean up the temporary file
    os.remove(temp_scadfile)
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

cad_meta.__tostring = function(obj)
  local info = {}

  -- Try to determine the object type from its SCAD content
  if obj.scad_content then
    local content = obj.scad_content

    -- Extract actual shape type based on content analysis
    local type = "unknown"

    -- Parse the most significant shape aspect from the content
    -- First look for CSG operations
    if content:match("difference") then
      type = "difference"
    elseif content:match("union") then
      type = "union"
    elseif content:match("intersection") then
      type = "intersection"
    elseif content:match("hull") then
      type = "hull"
    elseif content:match("minkowski") then
      type = "minkowski"
    -- Check for primitives
    elseif content:match("cube") then
      type = "cube"
    elseif content:match("sphere") then
      type = "sphere"
    elseif content:match("cylinder") then
      type = "cylinder"
    elseif content:match("polyhedron") then
      type = "polyhedron"
    elseif content:match("circle") then
      type = "circle"
    elseif content:match("square") then
      type = "square"
    elseif content:match("polygon") then
      type = "polygon"
    elseif content:match("text") then
      type = "text"
    elseif content:match("import") then
      type = "import"
    -- Check for common transformations
    elseif content:match("translate") then
      type = "translated"
    elseif content:match("scale") then
      type = "scaled"
    elseif content:match("rotate") then
      type = "rotated"
    elseif content:match("mirror") then
      type = "mirrored"
    elseif content:match("color") then
      type = "colored"
    end

    table.insert(info, type)
  end

  -- Add shape-specific parameters if available
  if obj.cylinder_params then
    local params = obj.cylinder_params
    local param_str = "h=" .. params.h

    -- Add radius or diameter info
    if params.r then
      param_str = param_str .. ", r=" .. params.r
    elseif params.r1 and params.r2 then
      if params.r1 == params.r2 then
        param_str = param_str .. ", r=" .. params.r1
      else
        param_str = param_str .. ", r1=" .. params.r1 .. ", r2=" .. params.r2
      end
    elseif params.d then
      param_str = param_str .. ", d=" .. params.d
    elseif params.d1 and params.d2 then
      if params.d1 == params.d2 then
        param_str = param_str .. ", d=" .. params.d1
      else
        param_str = param_str .. ", d1=" .. params.d1 .. ", d2=" .. params.d2
      end
    end

    -- Add center info if true
    if params.center then
      param_str = param_str .. ", center=true"
    end

    table.insert(info, param_str)
  end

  -- Add segments information
  if obj.segments and obj.segments ~= cad.settings.default_segments then
    table.insert(info, "segments: " .. obj.segments)
  end

  -- Add color information if set to non-default
  if
    obj.color and (obj.color[1] ~= 1 or obj.color[2] ~= 1 or obj.color[3] ~= 1)
  then
    local r, g, b = obj.color[1], obj.color[2], obj.color[3]

    -- Try to identify named colors
    local color_name = nil
    for name, rgb in pairs(cad._helpers.color_rgb) do
      if
        math.abs(r - rgb[1]) < 0.01
        and math.abs(g - rgb[2]) < 0.01
        and math.abs(b - rgb[3]) < 0.01
      then
        color_name = name
        break
      end
    end

    if color_name then
      table.insert(info, "color: " .. color_name)
    else
      table.insert(info, string.format("color: [%.1f, %.1f, %.1f]", r, g, b))
    end
  end

  -- Join all info with commas
  return table.concat(info, ", ")
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

function isArrayOnly(t)
  local count = 0
  for _ in pairs(t) do
    count = count + 1
  end
  return count == #t
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
  isArrayOnly = isArrayOnly,
}

-- Load other modules
require("lib_cad_2d")
require("lib_cad_3d")
require("lib_cad_transform")
require("lib_cad_export")
require("lib_cad_stl")
