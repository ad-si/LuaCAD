--[[--================================================================================
  Lua CAD Transform Functions
  Provides transformation operations for CAD objects
--]]
--================================================================================

-- This module will be loaded by src.core.cad_core.lua
local cad = cad or {}

--[[--------------------------------------------------------------------------------
  CAD TRANSFORMING OBJECT FUNCTIONS
--]]
--------------------------------------------------------------------------------

--[[----------------------------------------
  function <cad>:union()

  union the cad object
--]]
----------------------------------------
function cad._helpers.cad_meta.__index.union(obj_1)
  local obj = cad._helpers.cad_obj()
  cad._helpers.update_content(obj, "union() {\n")
  cad._helpers.indent_content(obj, 1)
  cad._helpers.update_content(obj, obj_1.scad_content)
  cad._helpers.indent_content(obj, -1)
  cad._helpers.update_content(obj, "}\n")
  obj_1.scad_content = obj.scad_content
  return obj_1
end

--[[----------------------------------------
  function <cad>:sub(obj_1 [, obj_2 [, ..., obj_n] ])

  substract multiple objects from the current object
--]]
----------------------------------------
function cad._helpers.cad_meta.__index.sub(obj_1, ...)
  local obj = cad._helpers.cad_obj()
  local obj_s = { ... }
  cad._helpers.update_content(obj, "difference() {\n")
  cad._helpers.indent_content(obj, 1)
  cad._helpers.update_content(
    obj,
    cad._helpers.cad_meta.__index.union(obj_1).scad_content
  )
  for i, v in ipairs(obj_s) do
    cad._helpers.update_content(obj, v.scad_content)
  end
  cad._helpers.indent_content(obj, -1)
  cad._helpers.update_content(obj, "}\n")
  obj_1.scad_content = obj.scad_content
  return obj_1
end

--[[----------------------------------------
  function <cad>:add(obj_1 [, obj_2 [, ..., obj_n] ])

  add multiple objects to the current object
--]]
----------------------------------------
function cad._helpers.cad_meta.__index.add(obj_1, ...)
  local obj = cad._helpers.cad_obj()
  cad._helpers.update_content(obj, obj_1.scad_content)
  local obj_s = { ... }
  for i, v in ipairs(obj_s) do
    cad._helpers.update_content(obj, v.scad_content)
  end
  obj_1.scad_content = obj.scad_content
  return obj_1
end

--[[----------------------------------------
  function <cad>:intersect(obj_1 [, obj_2 [, ..., obj_n] ])

  intersect multiple objects with the current object
--]]
----------------------------------------
function cad._helpers.cad_meta.__index.intersect(obj_1, ...)
  local obj = cad._helpers.cad_obj()
  local obj_s = { ... }
  cad._helpers.update_content(obj, "intersection() {\n")
  cad._helpers.indent_content(obj, 1)
  cad._helpers.update_content(obj, obj_1.scad_content)
  for i, v in ipairs(obj_s) do
    cad._helpers.update_content(obj, v.scad_content)
  end
  cad._helpers.indent_content(obj, -1)
  cad._helpers.update_content(obj, "}\n")
  obj_1.scad_content = obj.scad_content
  return obj_1
end

--[[----------------------------------------
  fuction <cad>:scale(x,y,z)

  scale an object in percent/100, 1=same size, 2=double, 0.5=half
--]]
----------------------------------------
function cad._helpers.cad_meta.__index.scale(obj_1, argsOrX, y, z)
  if type(argsOrX) == "table" then
    if argsOrX.v then
      x, y, z = argsOrX.v[1], argsOrX.v[2], argsOrX.v[3]
    elseif #argsOrX == 3 then
      x, y, z = argsOrX[1], argsOrX[2], argsOrX[3]
    elseif argsOrX.x then
      x, y, z = argsOrX.x, argsOrX.y, argsOrX.z
    end
  else
    x = argsOrX
  end

  local obj = cad._helpers.cad_obj()
  cad._helpers.update_content(
    obj,
    "scale(["
      .. tostring(valOrName(x or 0))
      .. ","
      .. tostring(valOrName(y or 0))
      .. ","
      .. tostring(valOrName(z or 0))
      .. "]) {\n"
  )
  cad._helpers.indent_content(obj, 1)
  cad._helpers.update_content(obj, obj_1.scad_content)
  cad._helpers.indent_content(obj, -1)
  cad._helpers.update_content(obj, "}\n")
  obj_1.scad_content = obj.scad_content
  return obj_1
end

--[[----------------------------------------
  fuction <cad>:resize(x,y,z)

  resize an object in x,y,z if any of the axes is zero will automatically scale
--]]
----------------------------------------
function cad._helpers.cad_meta.__index.resize(
  obj_1,
  x,
  y,
  z,
  auto_x,
  auto_y,
  auto_z
)
  local auto_x = (not auto_x and false) or true
  local auto_y = (not auto_y and false) or true
  local auto_z = (not auto_z and false) or true
  local obj = cad._helpers.cad_obj()
  cad._helpers.update_content(
    obj,
    "resize(["
      .. x
      .. ","
      .. y
      .. ","
      .. z
      .. "], auto=["
      .. tostring(auto_x)
      .. ","
      .. tostring(auto_y)
      .. ","
      .. tostring(auto_z)
      .. "]) {\n"
  )
  cad._helpers.indent_content(obj, 1)
  cad._helpers.update_content(obj, obj_1.scad_content)
  cad._helpers.indent_content(obj, -1)
  cad._helpers.update_content(obj, "}\n")
  obj_1.scad_content = obj.scad_content
  return obj_1
end

--[[------------------------------------------
  <cad>:translate(x, y [, z]) or
  <cad>:translate({x=x, y=y, z=z}) or
  <cad>:translate({x, y, z}) or
  <cad>:translate({v = {x, y, z}})

  z is by default 0.
  move the objects relative by x,y and z
--]]
------------------------------------------
function cad._helpers.cad_meta.__index.translate(obj_1, param1, param2, param3)
  local x, y, z = 0, 0, 0

  -- Handle table-style parameter
  if type(param1) == "table" and param1.tableType ~= "var" then
    if param1.v ~= nil then
      -- Vector-style: translate({v = {x, y, z}})
      x = param1.v[1] or 0
      y = param1.v[2] or 0
      z = param1.v[3] or 0
    elseif param1.x ~= nil then
      -- Object-style: translate({x=x, y=y, z=z})
      x = param1.x or 0
      y = param1.y or 0
      z = param1.z or 0
    else
      -- Array-style: translate({x, y, z})
      x = param1[1] or 0
      y = param1[2] or 0
      z = param1[3] or 0
    end
  else
    -- Handle direct parameters: translate(x, y, z)
    x = param1 or 0
    y = param2 or 0
    z = param3 or 0
  end

  local obj = cad._helpers.cad_obj()
  cad._helpers.update_content(
    obj,
    "translate(["
      .. valOrName(x)
      .. ","
      .. valOrName(y)
      .. ","
      .. valOrName(z)
      .. "]) {\n"
  )
  cad._helpers.indent_content(obj, 1)
  cad._helpers.update_content(obj, obj_1.scad_content)
  cad._helpers.indent_content(obj, -1)
  cad._helpers.update_content(obj, "}\n")
  obj_1.scad_content = obj.scad_content
  return obj_1
end

--[[------------------------------------------
  function <cad>:mirror(x, y, z)

  mirror the objects according to setup vector plane defined via x,y,z
  z is 0 by default
  values 1 or 0
--]]
------------------------------------------
function cad._helpers.cad_meta.__index.mirror(obj_1, x, y, z)
  local z = z or 0
  local obj = cad._helpers.cad_obj()
  cad._helpers.update_content(
    obj,
    "mirror([" .. x .. "," .. y .. "," .. z .. "]) {\n"
  )
  cad._helpers.indent_content(obj, 1)
  cad._helpers.update_content(obj, obj_1.scad_content)
  cad._helpers.indent_content(obj, -1)
  cad._helpers.update_content(obj, "}\n")
  obj_1.scad_content = obj.scad_content
  return obj_1
end

--[[------------------------------------------
  function <cad>:rotate(x, y, z, x_angle, y_angle, z_angle)

  x,y and z are the rotation point
--]]
------------------------------------------
function cad._helpers.cad_meta.__index.rotate(
  obj_1,
  x_or_angle,
  y_or_y_angle,
  z_or_z_angle,
  x_angle,
  y_angle,
  z_angle
)
  local obj = cad._helpers.cad_obj()

  -- Check if this is a simple rotation (just angles, no center point)
  if x_angle == nil then
    -- Simple rotation: rotate(x_angle, y_angle, z_angle)
    local x_angle = x_or_angle or 0
    local y_angle = y_or_y_angle or 0
    local z_angle = z_or_z_angle or 0

    cad._helpers.update_content(
      obj,
      "rotate([" .. x_angle .. "," .. y_angle .. "," .. z_angle .. "]) {\n"
    )
  else
    -- Full rotation with center point: rotate(x, y, z, x_angle, y_angle, z_angle)
    local x = x_or_angle or 0
    local y = y_or_y_angle or 0
    local z = z_or_z_angle or 0

    cad._helpers.update_content(
      obj,
      "translate(["
        .. x
        .. ","
        .. y
        .. ","
        .. z
        .. "])\nrotate(["
        .. x_angle
        .. ","
        .. y_angle
        .. ","
        .. z_angle
        .. "])\ntranslate(["
        .. -x
        .. ","
        .. -y
        .. ","
        .. -z
        .. "]) {\n"
    )
  end
  cad._helpers.indent_content(obj, 1)
  cad._helpers.update_content(obj, obj_1.scad_content)
  cad._helpers.indent_content(obj, -1)
  cad._helpers.update_content(obj, "}\n")
  obj_1.scad_content = obj.scad_content
  return obj_1
end

--[[------------------------------------------
  function <cad>:rotate2d(x, y, angle)
  function rotate2d_end()

  x and y are the rotation point
--]]
------------------------------------------
function cad._helpers.cad_meta.__index.rotate2d(obj_1, x, y, angle)
  local obj = cad._helpers.cad_obj()
  cad._helpers.update_content(
    obj,
    "translate(["
      .. x
      .. ","
      .. y
      .. ",0])\nrotate([0,0,"
      .. angle
      .. "])\ntranslate(["
      .. -x
      .. ","
      .. -y
      .. ",0]) {\n"
  )
  cad._helpers.indent_content(obj, 1)
  cad._helpers.update_content(obj, obj_1.scad_content)
  cad._helpers.indent_content(obj, -1)
  cad._helpers.update_content(obj, "}\n")
  obj_1.scad_content = obj.scad_content
  return obj_1
end

--[[------------------------------------------
  function <cad>:rotateextrude(angle, convexity)

  the object has to moved away from the center for rotate_extrude to succeed
  angle: optional angle in degrees for partial rotation (default: 360)
  convexity: optional hint for OpenSCAD renderer (default: 10)
--]]
------------------------------------------
function cad._helpers.cad_meta.__index.rotateextrude(obj_1, angle, convexity)
  local obj = cad._helpers.cad_obj()
  local params = "()"

  if angle or convexity then
    params = "("
    if angle then
      params = params .. "angle = " .. angle
    end

    if angle and convexity then
      params = params .. ", "
    end

    if convexity then
      params = params .. "convexity = " .. convexity
    end
    params = params .. ")"
  end

  cad._helpers.update_content(obj, "rotate_extrude" .. params .. "\n{\n")
  cad._helpers.indent_content(obj, 1)
  cad._helpers.update_content(obj, obj_1.scad_content)
  cad._helpers.indent_content(obj, -1)
  cad._helpers.update_content(obj, "}\n")
  obj_1.scad_content = obj.scad_content
  return obj_1
end

--[[------------------------------------------
  function <cad>:linearextrude(depth [, twist [, slices [, scale] ] ])

  note linear extrude moves the object back to the ground
--]]
------------------------------------------
function cad._helpers.cad_meta.__index.linearextrude(
  obj_1,
  depth,
  twist,
  slices,
  scale
)
  local twist = twist or 0 -- in degrees
  local slices = slices or 1 -- in how many slices it should be put
  local scale = scale or 1.0 -- if the object should be scaled towards extrusion
  local obj = cad._helpers.cad_obj()
  cad._helpers.update_content(
    obj,
    "linear_extrude(height = "
      .. depth
      .. ", twist = "
      .. twist
      .. ", slices = "
      .. slices
      .. ", scale = "
      .. scale
      .. ", center = false) {\n"
  )
  cad._helpers.indent_content(obj, 1)
  cad._helpers.update_content(obj, obj_1.scad_content)
  cad._helpers.indent_content(obj, -1)
  cad._helpers.update_content(obj, "}\n")
  obj_1.scad_content = obj.scad_content
  return obj_1
end

--[[------------------------------------------
  function <cad>:hull()

  hull the cad object
--]]
------------------------------------------
function cad._helpers.cad_meta.__index.hull(obj_1)
  local obj = cad._helpers.cad_obj()
  cad._helpers.update_content(obj, "hull() {\n")
  cad._helpers.indent_content(obj, 1)
  cad._helpers.update_content(obj, obj_1.scad_content)
  cad._helpers.indent_content(obj, -1)
  cad._helpers.update_content(obj, "}\n")
  obj_1.scad_content = obj.scad_content
  return obj_1
end

--[[------------------------------------------
  function <cad>:minkowski()

  apply minkowski operation to the cad object
--]]
------------------------------------------
function cad._helpers.cad_meta.__index.minkowski(obj_1)
  local obj = cad._helpers.cad_obj()
  cad._helpers.update_content(obj, "minkowski() {\n")
  cad._helpers.indent_content(obj, 1)
  cad._helpers.update_content(obj, obj_1.scad_content)
  cad._helpers.indent_content(obj, -1)
  cad._helpers.update_content(obj, "}\n")
  obj_1.scad_content = obj.scad_content
  return obj_1
end

--[[------------------------------------------
  function <cad>:offset(d, chamfer)

  Offset the object with distance d
--]]
------------------------------------------
function cad._helpers.cad_meta.__index.offset(obj_1, d, chamfer)
  local obj = cad._helpers.cad_obj()
  local chamfer = (chamfer and true) or false
  cad._helpers.update_content(
    obj,
    "offset(d=" .. d .. ",chamfer=" .. tostring(chamfer) .. ") {\n"
  )
  cad._helpers.indent_content(obj, 1)
  cad._helpers.update_content(obj, obj_1.scad_content)
  cad._helpers.indent_content(obj, -1)
  cad._helpers.update_content(obj, "}\n")
  obj_1.scad_content = obj.scad_content
  return obj_1
end

--[[------------------------------------------
  function <cad>:offsetradius(r, chamfer)

  Offset the object with radius r
--]]
------------------------------------------
function cad._helpers.cad_meta.__index.offsetradius(obj_1, r, chamfer)
  local obj = cad._helpers.cad_obj()
  local chamfer = (chamfer and true) or false
  cad._helpers.update_content(
    obj,
    "offset(r=" .. r .. ",chamfer=" .. tostring(chamfer) .. ") {\n"
  )
  cad._helpers.indent_content(obj, 1)
  cad._helpers.update_content(obj, obj_1.scad_content)
  cad._helpers.indent_content(obj, -1)
  cad._helpers.update_content(obj, "}\n")
  obj_1.scad_content = obj.scad_content
  return obj_1
end

--[[------------------------------------------
  function <cad>:multmatrix(matrix)

  Apply a 4x4 transformation matrix to the object
  matrix should be a table with 16 elements representing a 4x4 matrix in row-major order
--]]
------------------------------------------
function cad._helpers.cad_meta.__index.multmatrix(obj_1, matrix)
  -- Validate matrix has 16 elements
  if #matrix ~= 16 then
    error("multmatrix requires a 4x4 matrix (16 elements)")
  end

  local obj = cad._helpers.cad_obj()

  -- Format the matrix in OpenSCAD format [[a,b,c,d], [e,f,g,h], [i,j,k,l], [m,n,o,p]]
  local matrix_str = "["
  for i = 0, 3 do
    matrix_str = matrix_str .. "["
    for j = 0, 3 do
      matrix_str = matrix_str .. matrix[i * 4 + j + 1]
      if j < 3 then
        matrix_str = matrix_str .. ","
      end
    end
    matrix_str = matrix_str .. "]"
    if i < 3 then
      matrix_str = matrix_str .. ","
    end
  end
  matrix_str = matrix_str .. "]"

  cad._helpers.update_content(obj, "multmatrix(" .. matrix_str .. ") {\n")
  cad._helpers.indent_content(obj, 1)
  cad._helpers.update_content(obj, obj_1.scad_content)
  cad._helpers.indent_content(obj, -1)
  cad._helpers.update_content(obj, "}\n")
  obj_1.scad_content = obj.scad_content
  return obj_1
end

--[[------------------------------------------
  function <cad>:projection(cut)

  Creates a 2D projection of a 3D object
  cut = true/false (default false) - Sets whether to project a cross-section (cut=true)
--]]
------------------------------------------
function cad._helpers.cad_meta.__index.projection(obj_1, cut)
  local obj = cad._helpers.cad_obj()
  local cut_value = cut or false
  cad._helpers.update_content(
    obj,
    "projection(cut = " .. tostring(cut_value) .. ") {\n"
  )
  cad._helpers.indent_content(obj, 1)
  cad._helpers.update_content(obj, obj_1.scad_content)
  cad._helpers.indent_content(obj, -1)
  cad._helpers.update_content(obj, "}\n")
  obj_1.scad_content = obj.scad_content
  return obj_1
end
