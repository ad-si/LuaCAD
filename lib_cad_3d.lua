--[[--================================================================================
  Lua CAD 3D Functions
  Provides 3D object creation primitives
--]]
--================================================================================

-- This module will be loaded by lib_cad_core.lua
local cad = cad or {}

--[[----------------------------------------------------------------------------
  3D OBJECTS
--]]
----------------------------------------------------------------------------

--[[------------------------------------------
  cad.sphere{r = radius, d = diameter}

  Draw a sphere
--]]
------------------------------------------
local scad_sphere_radius = [[
sphere(r = $RADIUS);
]]
local scad_sphere_diameter = [[
sphere(d = $DIAMETER);
]]
function cad.sphere(args)
  local obj = cad._helpers.cad_obj()
  if args.r then
    local t = { RADIUS = valOrName(args.r) }
    cad._helpers.update_content_t(obj, scad_sphere_radius, t)
    return obj
  elseif args.d then
    local t = { DIAMETER = valOrName(args.d) }
    cad._helpers.update_content_t(obj, scad_sphere_diameter, t)
    return obj
  else
    error("No radius or diameter was given")
  end
end

--[[------------------------------------------
  cad.cube{size = {x, y, z}, center = true/false}

  Draw a cube or a cuboid
--]]
------------------------------------------
local scad_cube = [[
translate([$X, $Y, $Z])
cube([$WIDTH, $HEIGHT, $DEPTH]);
]]
function cad.cube(args)
  local xLen = args.size[1] or 1
  local yLen = args.size[2] or 1
  local zLen = args.size[3] or 1
  local isCenter = args.center or false

  local obj = cad._helpers.cad_obj()
  local t = {
    X = (isCenter and -xLen / 2) or 0,
    Y = (isCenter and -yLen / 2) or 0,
    Z = (isCenter and -zLen / 2) or 0,
    WIDTH = xLen,
    HEIGHT = zLen,
    DEPTH = yLen,
  }
  cad._helpers.update_content_t(obj, scad_cube, t)
  return obj
end

--[[------------------------------------------
  cad.cylinder{h = height, r = radius, d = diameter, r1 = bottom_radius, r2 = top_radius, d1 = bottom_diameter, d2 = top_diameter, center = true/false}

  Draw a cylinder or cone
--]]
------------------------------------------
local scad_cylinder_standard = [[
translate([$X,$Y,$Z])
cylinder(h = $H, r1 = $R1, r2 = $R2, center = $CENTER);
]]
local scad_cylinder_diameter = [[
translate([$X,$Y,$Z])
cylinder(h = $H, d1 = $D1, d2 = $D2, center = $CENTER);
]]
function cad.cylinder(args)
  local obj = cad._helpers.cad_obj()
  local isCenter = args.center or false
  local height = args.h or 1
  local translateZ = (isCenter and -height / 2) or 0

  if args.d or args.d1 or args.d2 then
    -- Use diameter version
    local d1 = args.d1 or args.d or 1
    local d2 = args.d2 or args.d or d1
    local t = {
      X = 0,
      Y = 0,
      Z = translateZ,
      H = height,
      D1 = d1,
      D2 = d2,
      CENTER = tostring(isCenter),
    }
    cad._helpers.update_content_t(obj, scad_cylinder_diameter, t)
  else
    -- Use radius version
    local r1 = args.r1 or args.r or 0.5
    local r2 = args.r2 or args.r or r1
    local t = {
      X = 0,
      Y = 0,
      Z = translateZ,
      H = height,
      R1 = r1,
      R2 = r2,
      CENTER = tostring(isCenter),
    }
    cad._helpers.update_content_t(obj, scad_cylinder_standard, t)
  end

  return obj
end

--[[------------------------------------------
  function cad.polyhedron(x,y,z, t_points, t_faces)

  draw a polygon from given points {{x,y,z},{x,y,z},...,{x,y,z}}
  t_faces: vector of point n-tuples with n >= 3. Each number is the 0-indexed point number from the point vector.
    That is, faces=[ [0,1,4] ] specifies a triangle made from the first, second, and fifth point listed in points.
    When referencing more than 3 points in a single tuple, the points must all be on the same plane.
  note angles are the outer angles measured
--]]
------------------------------------------
local scad_polyhedron = [[
translate([$X,$Y,$Z])
polyhedron($COORDINATES, faces=$FACES);
]]
function cad.polyhedron(x, y, z, t_points, t_faces)
  local obj = cad._helpers.cad_obj()
  -- create coordinates string
  local coor = "["
  for i, v in ipairs(t_points) do
    coor = coor .. "[" .. v[1] .. "," .. v[2] .. "," .. v[3] .. "],"
  end
  coor = string.sub(coor, 1, -2) .. "]"
  local faces = "["
  for i, v in ipairs(t_faces) do
    faces = faces .. "[" .. table.concat(v, ",") .. "],"
  end
  faces = string.sub(faces, 1, -2) .. "]"
  local t = { X = x, Y = y, Z = z, COORDINATES = coor, FACES = faces }
  cad._helpers.update_content_t(obj, scad_polyhedron, t)
  return obj
end

--[[------------------------------------------
  cad.rotate_extrude{angle = angle, convexity = convexity}

  Rotates a 2D shape around the Z-axis to form a solid
  - angle: optional angle in degrees for partial rotation (default: 360)
  - convexity: optional hint for OpenSCAD renderer (default: 10)

  Note: The 2D shape must be on the positive X side of the Y axis.
--]]
------------------------------------------
function cad.rotate_extrude(args)
  local obj = cad._helpers.cad_obj()
  local params = "()"

  if args.angle or args.convexity then
    params = "("
    local paramCount = 0

    if args.angle then
      params = params .. "angle = " .. args.angle
      paramCount = paramCount + 1
    end

    if args.convexity then
      if paramCount > 0 then
        params = params .. ", "
      end
      params = params .. "convexity = " .. args.convexity
    end

    params = params .. ")"
  end

  cad._helpers.update_content(obj, "rotate_extrude" .. params .. "\n{\n")
  cad._helpers.intend_content(obj, 1)
  -- This will be empty until objects are added to it
  cad._helpers.intend_content(obj, -1)
  cad._helpers.update_content(obj, "}\n")

  return obj
end

--[[--------------------------------------------------------------------------------
  GEOMETRIC 3D OBJECTS
--]]
--------------------------------------------------------------------------------

--[[----------------------------------------
  function cad.pyramid(x,y,z, length, height)

  draws a pyramid with length as bottom side length and height
--]]
----------------------------------------
function cad.pyramid(x, y, z, length, height)
  local t_points = {
    { 0, 0, 0 },
    { length, 0, 0 },
    { length, length, 0 },
    { 0, length, 0 },
    { length / 2, length / 2, height },
  }
  local t_faces = {
    { 0, 1, 4 },
    { 1, 2, 4 },
    { 2, 3, 4 },
    { 3, 0, 4 },
    { 3, 2, 1, 0 },
  }
  local obj = cad.polyhedron(x, y, z, t_points, t_faces)
  return obj
end
