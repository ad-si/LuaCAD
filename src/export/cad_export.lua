--[[--================================================================================
  Lua CAD Export Functions
  Provides export functionality for CAD objects
--]]
--================================================================================

-- This module will be loaded by src.core.cad_core.lua
local cad = cad or {}

--[[----------------------------------------
  function <cad>:export(file [, color [, verbose] ])
  function <cad>:export(<file>.svg [, {width,height,{color_fill},{color_stroke},stroke_width} [, verbose] ])

  Export 2D: dxf, svg
  Export 3D: stl
  Export 3D color: wrl, x3d, obj

  Color is 8bit RGB
  color[R,G,B] = {[0,1], [0,1], [0,1]}

  Export to dxf, svg, stl or obj with color color = {r,g,b}
--]]
----------------------------------------
local t_ext = {
  -- 2D
  [".dxf"] = function(obj, file)
    cad._helpers.export_scad(obj, file)
  end,
  [".svg"] = function(obj, file)
    cad._helpers.export_scad(obj, file)
    local color = {
      math.floor(obj.color[1] * 255),
      math.floor(obj.color[2] * 255),
      math.floor(obj.color[3] * 255),
    }
    if color then
      local w = color[1] or "100%%"
      local h = color[2] or "100%%"
      local fill = color[3] or { 0, 0, 255 }
      local stroke = color[4] or { 0, 0, 0 }
      local stroke_width = color[5] or 0.0
      local svg = io.open(file, "r")
      local content = svg:read("*a")
      svg:close()
      --content = string.gsub(content, "<title>OpenSCAD Model</title>\n", "")
      content = string.gsub(content, '%sheight="%d+"', ' height="' .. h .. '"')
      content = string.gsub(content, '%swidth="%d+"', ' width="' .. w .. '"')
      content = string.gsub(
        content,
        'fill="lightgray"',
        'fill="#'
          .. string.format("%02X%02X%02X", fill[1], fill[2], fill[3])
          .. '"'
      )
      content = string.gsub(
        content,
        'stroke="black"',
        'stroke="#'
          .. string.format("%02X%02X%02X", stroke[1], stroke[2], stroke[3])
          .. '"'
      )
      content = string.gsub(
        content,
        'stroke%-width="0%.5"',
        'stroke-width="' .. stroke_width .. '"'
      )
      local svg = io.open(file, "w")
      svg:write(content)
      svg:close()
    end
  end,
  -- 3D
  [".scad"] = function(obj, file)
    cad._helpers.export_scad(obj, file)
  end,
  [".stl"] = function(obj, file)
    cad._helpers.export_scad(obj, file)
  end,
  [".obj"] = function(obj, file)
    -- export as stl
    cad._helpers.export_scad(obj, file .. ".stl")
    local color = color or "white"
    cad.stl.toobj(file, { file .. ".stl" }, { obj.color })
    os.remove(file .. ".stl")
  end,
  [".wrl"] = function(obj, file)
    -- export as stl
    cad._helpers.export_scad(obj, file .. ".stl")
    local color = color or "white"
    cad.stl.towrl(file, { file .. ".stl" }, { obj.color })
    os.remove(file .. ".stl")
  end,
}

-- Define the function at the top level
local get_export_script_path

function cad._helpers.cad_meta.__index.export(obj, file, verbose)
  -- For example files like car.lua, the path is already relative to the right directory,
  -- so we don't need to modify it. Only cad.export() should prepend the script directory.

  if verbose and not isTesting then
    print("---------------------------------")
    print("cad.export: -> " .. file)
    print()
    if cad.openscad and cad.openscad.include then
      print(cad.openscad.include)
    end
    print("$fn = " .. obj.segments .. ";")
    print(obj.scad_content)
    print()
    local scadfile = file .. ".scad"
    print("scad_file: " .. scadfile)
    print("---------------------------------")
  end

  -- check extension
  local ext = cad._helpers.get_extension(file)
  local f = t_ext[ext]
  if not f then
    error("<cad>:export, unknown extension " .. ext)
  end

  -- Resolve relative paths for display
  local display_path = file
  if file:sub(1, 1) ~= "/" and file:sub(1, 1) ~= "\\" then
    local script_dir = get_export_script_path() or ""
    if script_dir ~= "" and script_dir ~= "./" then
      display_path = script_dir .. file
    end
  end

  -- Don't log during testing
  if not isTesting then
    print("Exporting to " .. display_path)
  end

  f(obj, file)
  return obj
end

--[[----------------------------------------
  function cad.export(filename, obj_1 [, obj_2, obj_n])
  exports all objects into one file

  Export 2D: dxf, svg
  Export 3D: scad, stl
  Export 3D color: wrl, x3d, obj

  Export to dxf, svg, stl or obj with color color = {r,g,b}
--]]
----------------------------------------
local f_ext = {
  -- 2D
  [".dxf"] = function(file, objs)
    local obj = objs[1]
    for i = 2, #objs do
      obj:add(objs[i])
    end
    obj:export(file)
  end,
  [".svg"] = function(file, objs)
    local obj = objs[1]
    for i = 2, #objs do
      obj:add(objs[i])
    end
    obj:export(file)
  end,
  [".scad"] = function(file, objs)
    local obj = objs[1]
    for i = 2, #objs do
      obj:add(objs[i])
    end
    obj:export(file)
  end,
  [".stl"] = function(file, objs)
    local obj = objs[1]
    for i = 2, #objs do
      obj:add(objs[i])
    end
    obj:export(file)
  end,
  [".obj"] = function(file, objs)
    local t_files = {}
    local t_colors = {}
    for i, v in ipairs(objs) do
      local fname = cad.settings.tmpfile .. "_" .. i .. ".stl"
      v:export(fname)
      table.insert(t_files, fname)
      table.insert(t_colors, v.color)
    end
    cad.stl.toobj(file, t_files, t_colors)
    for i, v in ipairs(t_files) do
      os.remove(v)
    end
  end,
  [".wrl"] = function(file, objs)
    local t_files = {}
    local t_colors = {}
    for i, v in ipairs(objs) do
      local fname = cad.settings.tmpfile .. "_" .. i .. ".stl"
      v:export(fname)
      table.insert(t_files, fname)
      table.insert(t_colors, v.color)
    end
    cad.stl.towrl(file, t_files, t_colors)
    for i, v in ipairs(t_files) do
      os.remove(v)
    end
  end,
}

-- Helper function to get script path
get_export_script_path = function()
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

function cad.export(filename, ...)
  -- If file is not an absolute path, make it relative to the script path
  if filename:sub(1, 1) ~= "/" and filename:sub(1, 1) ~= "\\" then
    local script_dir = get_export_script_path() or ""
    filename = script_dir .. filename
  end

  local objs = { ... }
  local ext = cad._helpers.get_extension(filename)
  local f = f_ext[ext]
  if not f then
    error("cad.export, unknown extension " .. ext)
  end

  -- Check if any objects have showOnly flag set
  local showOnlyObjs = {}
  for _, obj in ipairs(objs) do
    if obj.showOnly then
      table.insert(showOnlyObjs, obj)
    end
  end

  -- If there are objects with showOnly flag, only export those
  if #showOnlyObjs > 0 then
    f(filename, showOnlyObjs)
  else
    f(filename, objs)
  end
end
