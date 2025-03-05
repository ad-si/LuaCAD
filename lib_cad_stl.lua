--[[--================================================================================
  Lua CAD STL Functions
  Provides STL-specific functionality for CAD objects
--]]
--================================================================================

--[[--------------------------------------------------------------------------------
  Author: Michael Lutz
  Licensed under the same terms as Lua itself
--]]
--------------------------------------------------------------------------------

-- This module will be loaded by lib_cad_core.lua
local cad = cad or {}

--[[--------------------------------------------------------------------------------
  STL FUNCTIONS
--]]
--------------------------------------------------------------------------------

cad.stl = {}

function cad.stl.write(filename, s_stl)
  local f, err = io.open(filename, "w")
  if err then
    error(err)
  end
  f:write(s_stl)
  f:close()
end

--/////////////////////////////////////////////////////////////////
-- import ASCII STL file
-- stl files consist of a normal and three points aranged in counter clock
-- returns all the faces {{normal,v1,v2,v3},...}
-- where normal and v... are a table holding three floating point numbers e.g. {num1,num2,num3}
-- does not perform any checking on the file itself
local function cad_import_stl(filename)
  local file_stl, err = io.open(filename, "r")
  if err then
    error(err)
  end
  local t_faces = {}
  local cur_face = {}
  local count = 0
  for line in file_stl:lines() do
    if string.find(line, "^%s*facet normal") then
      local x, y, z = string.match(line, "(%S+)%s(%S+)%s(%S+)$")
      table.insert(cur_face, { tonumber(x), tonumber(y), tonumber(z) })
    end
    if string.find(line, "^%s*vertex") then
      local x, y, z = string.match(line, "(%S+)%s(%S+)%s(%S+)$")
      table.insert(cur_face, { tonumber(x), tonumber(y), tonumber(z) })
      count = count + 1
      if count == 3 then
        table.insert(t_faces, cur_face)
        cur_face = {}
        count = 0
      end
    end
  end
  file_stl:close()
  return t_faces
end

--[[
  Convert ASCII STL file to OBJ (wavefront file) with optional coloring
  Info:

  Imports an ASCII stl file into a t_faces structure
  Exports a t_faces structure into an OBJ file with optional coloring
--]]
--/////////////////////////////////////////////////////////////////
-- export t_faces structure to OBJ (wavefront) file
-- filename: full filepath to obj file
-- t_faces a t_faces structure
-- color is optional and has to be a table holding three r,g,b, numbers e.g. {[0,1],[0,1],[0,1]} or a string e.g. "white"
function cad.stl.toobj(obj_filename, t_stl_filenames, t_colors)
  -- strip only filename
  local filename = string.match(obj_filename, "([^\\/]+)%.%S%S%S$")

  -- if color then write mtl
  local mtl_filename = filename .. ".mtl"
  local file, err = io.open(mtl_filename, "w")
  if err then
    error(err)
  end
  local t_mtl_color = {}
  for i, v in ipairs(t_colors) do
    --print(v)
    local col = {}
    if cad._helpers.color_rgb[v] then
      color = cad._helpers.color_rgb[v]
    else
      color = { v[1], v[2], v[3] }
    end
    local mtl_color = "color_["
      .. string.format("%.06f-%.06f-%.06f", color[1], color[2], color[3])
      .. "]_"
      .. i
    file:write(
      "# Color: "
        .. mtl_color
        .. "\n"
        .. "newmtl "
        .. mtl_color
        .. "\n"
        .. "Ka "
        .. string.format("%.06f %.06f %.06f", color[1], color[2], color[3])
        .. "\n"
        .. "Kd "
        .. string.format("%.06f %.06f %.06f", color[1], color[2], color[3])
        .. "\n"
        .. "Ks 0.000000 0.000000 0.000000\n"
        .. "Ns 2.000000\n"
        .. "d 1.000000\n"
        .. "Tr 1.000000\n\n"
    )
    table.insert(t_mtl_color, mtl_color)
  end
  file:close()

  -- get all vertices
  local t_faces = {}
  for i, v in ipairs(t_stl_filenames) do
    table.insert(t_faces, cad_import_stl(v))
  end

  -- write obj

  local file, err = io.open(obj_filename, "w")
  if err then
    error(err)
  end

  file:write("#########################################\n")
  file:write("# Wavefront file\n")
  file:write("# Created via: Lua_CAD (https://github.com/mvlutz/Lua_CAD)\n")
  file:write("# Date: " .. os.date() .. "\n")
  file:write("#########################################\n\n")

  if color then
    file:write("#########################################\n")
    file:write("# MTL lib:\n\n")
    file:write("mtllib " .. mtl_filename .. "\n\n")
  end

  -- List of Vertices
  file:write("#########################################\n")
  file:write("# List of vertices:\n\n")

  for _, t_v in ipairs(t_faces) do
    for i, v in ipairs(t_v) do
      for i2 = 2, 4 do
        file:write(
          "v "
            .. string.format("%f", v[i2][1])
            .. " "
            .. string.format("%f", v[i2][2])
            .. " "
            .. string.format("%f", v[i2][3])
            .. "\n"
        )
      end
    end
  end
  file:write("\n")

  -- Normals in (x,y,z) form; normals might not be unit.
  -- Normals are not needed since stl saves the triangles in counter clock anyways
  --file:write("\n# List of Normals\n\n")
  --for i,v in ipairs(t_faces) do
  --  file:write("vn "..string.format("%f",v[1][1]).." "..string.format("%f",v[1][2]).." "..string.format("%f",v[1][3]).."\n")
  --end

  -- Face Definitions (see below)
  local count = 0
  for i, v in ipairs(t_mtl_color) do
    file:write("\n#########################################\n")
    file:write("# Face definitions:\n\n")

    if color then
      file:write("usemtl " .. v .. "\n\n")
    end

    for i, v in ipairs(t_faces[i]) do
      local i2 = count * 3
      --file:write("f "..(i2+1).."//"..i.." "..(i2+2).."//"..i.." "..(i2+3).."//"..i.."\n")
      file:write("f " .. (i2 + 1) .. " " .. (i2 + 2) .. " " .. (i2 + 3) .. "\n")
      count = count + 1
    end
  end
  file:close()
end

--/////////////////////////////////////////////////////////////////
-- export stl to wrl 2.0
-- filename: full filepath to obj file
-- color is optional and has to be a table holding three r,g,b, numbers e.g. {[0,1],[0,1],[0,1]} or a string e.g. "white"
function cad.stl.towrl(wrl_filename, t_stl_filenames, t_colors)
  -- strip only filename
  local filename = string.match(wrl_filename, "([^\\/]+)%.%S%S%S$")

  -- write wrl
  local file, err = io.open(wrl_filename, "w")
  if err then
    error(err)
  end

  -- set utf8 syntax
  file:write("#VRML V2.0 utf8\n")
  file:write("\n")
  file:write("# Created via: Lua_CAD (https://github.com/mvlutz/Lua_CAD)\n")
  file:write("# Date: " .. os.date() .. "\n")

  -- for each stl file write a shape
  -- get all vertices
  for i, v in ipairs(t_stl_filenames) do
    local t_faces = cad_import_stl(v)
    file:write("\nShape {\n")
    local col = t_colors[i]
    if col then
      if cad._helpers.color_rgb[col] then
        col = cad._helpers.color_rgb[v]
      else
        col = { col[1], col[2], col[3] }
      end
      file:write("\tappearance Appearance {\n")
      file:write("\t\tmaterial Material {\n")
      file:write(
        "\t\t\tdiffuseColor "
          .. string.format("%.06f %.06f %.06f", col[1], col[2], col[3])
          .. "\n"
      )
      file:write("\t\t}\n")
      file:write("\t}\n")
    end

    -- List of Vertices
    file:write("\tgeometry IndexedFaceSet {\n\n")
    file:write("#\t\tList of Coordinates:\n")
    file:write("\t\tcoord Coordinate {\n")
    file:write("\t\t\tpoint [\n")
    for i, v in ipairs(t_faces) do
      for i2 = 2, 4 do
        file:write(
          "\t\t\t\t"
            .. string.format("%f", v[i2][1])
            .. " "
            .. string.format("%f", v[i2][2])
            .. " "
            .. string.format("%f", v[i2][3])
            .. "\n"
        )
      end
    end
    file:write("\t\t\t]\n")
    file:write("\t\t}\n\n")
    file:write("#\t\tList of Faces:\n")
    file:write("\t\tcoordIndex [\n")
    local count = 0
    for _, t_v in ipairs(t_faces) do
      file:write(
        "\t\t\t" .. count .. " " .. (count + 1) .. " " .. (count + 2) .. " -1\n"
      )
      count = count + 3
    end
    file:write("\t\t]\n")
    file:write("\t}\n")
    file:write("}\n")
  end
  file:close()
end

function cad.stl.cube(x, y, z, width, height, depth)
  local w, h, d = width, height, depth

  local vertex = {
    { { -1, 0, 0 }, { 0, 0, d }, { 0, h, d }, { 0, 0, 0 } },
    { { -1, 0, 0 }, { 0, 0, 0 }, { 0, h, d }, { 0, h, 0 } },
    { { 0, 0, 1 }, { 0, 0, d }, { w, 0, d }, { w, h, d } },
    { { 0, 0, 1 }, { 0, h, d }, { 0, 0, d }, { w, h, d } },
    { { 0, -1, 0 }, { 0, 0, 0 }, { w, 0, 0 }, { w, 0, d } },
    { { 0, -1, 0 }, { 0, 0, d }, { 0, 0, 0 }, { w, 0, d } },
    { { 0, 0, -1 }, { 0, h, 0 }, { w, h, 0 }, { 0, 0, 0 } },
    { { 0, 0, -1 }, { 0, 0, 0 }, { w, h, 0 }, { w, 0, 0 } },
    { { 0, 1, 0 }, { 0, h, d }, { w, h, d }, { 0, h, 0 } },
    { { 0, 1, 0 }, { 0, h, 0 }, { w, h, d }, { w, h, 0 } },
    { { 1, 0, 0 }, { w, 0, 0 }, { w, h, 0 }, { w, h, d } },
    { { 1, 0, 0 }, { w, 0, d }, { w, 0, 0 }, { w, h, d } },
  }

  -- create stl string
  local indent = "  "
  local stl = "solid cube\n"
  for i, v in ipairs(vertex) do
    stl = stl
      .. indent
      .. "facet normal "
      .. v[1][1]
      .. " "
      .. v[1][2]
      .. " "
      .. v[1][3]
      .. "\n"
    stl = stl .. indent .. indent .. "outer loop\n"
    stl = stl
      .. indent
      .. indent
      .. indent
      .. "vertex "
      .. (x + v[2][1])
      .. " "
      .. (y + v[2][2])
      .. " "
      .. (z + v[2][3])
      .. "\n"
    stl = stl
      .. indent
      .. indent
      .. indent
      .. "vertex "
      .. (x + v[3][1])
      .. " "
      .. (y + v[3][2])
      .. " "
      .. (z + v[3][3])
      .. "\n"
    stl = stl
      .. indent
      .. indent
      .. indent
      .. "vertex "
      .. (x + v[4][1])
      .. " "
      .. (y + v[4][2])
      .. " "
      .. (z + v[4][3])
      .. "\n"
    stl = stl .. indent .. indent .. "endloop\n"
  end
  stl = stl .. "ensolid cube\n"

  return stl
end
