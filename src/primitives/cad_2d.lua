--[[--================================================================================
  Lua CAD 2D Functions
  Provides 2D object creation primitives
--]]
--================================================================================

-- This module will be loaded by src.core.cad_core.lua
local cad = cad or {}

--[[--------------------------------------------------------------------------------
  2D OBJECTS

  Only give x and y, z is obsolete since linear extrude moves the item back to zero z.
--]]
--------------------------------------------------------------------------------

--[[----------------------------------------
  function cad.rect(args)
  cad.square(args)

  draw rectangle with given size and center options
  args = {size = {width, height}, center = true/false}
--]]
----------------------------------------
local scad_rect = [[
// rect
translate([$X,$Y,0])
square([$WIDTH,$HEIGHT],center=$CENTER);
]]
function cad.rect(args)
  local obj = cad._helpers.cad_obj()

  local size = args.size or { 1, 1 }
  local width = size[1] or 1
  local height = size[2] or width
  local isCenter = args.center or false

  local x = (isCenter and -width / 2) or 0
  local y = (isCenter and -height / 2) or 0

  local t = {
    X = x,
    Y = y,
    WIDTH = width,
    HEIGHT = height,
    CENTER = tostring(isCenter),
  }
  cad._helpers.update_content_t(obj, scad_rect, t)

  return obj
end
cad.square = cad.rect

--[[------------------------------------------
  function cad.circle(args)

  draw circle
  args = {r = radius, d = diameter, center = true/false}
--]]
------------------------------------------
local scad_circle = [[
// circle
translate([$X,$Y,0])
circle($RORD);
]]
function cad.circle(args)
  local obj = cad._helpers.cad_obj()

  local radius, diameter
  local isCenter = args.center or false

  if args.r then
    radius = args.r
    diameter = nil
  elseif args.d then
    diameter = args.d
    radius = nil
  else
    error("No radius (r) or diameter (d) was given")
  end

  local x = (isCenter and 0) or 0
  local y = (isCenter and 0) or 0

  local rord = radius and ("r = " .. radius) or ("d = " .. diameter)

  local t = { X = x, Y = y, RORD = rord }
  cad._helpers.update_content_t(obj, scad_circle, t)
  return obj
end

--[[------------------------------------------
  cad.polygon{points = {{x1, y1}, {x2, y2}, ...}, paths = {{0,1,2}, {2,3,0}, ...}, convexity = N}

  Create a polygon with the given points and optional paths

  - points: Array of points in the form {{x1, y1}, {x2, y2}, ...}
  - paths: Optional array of point indices forming paths
  - convexity: Optional hint for OpenSCAD renderer

  Note: Points must be in counter-clockwise direction
  Special functions in points array {{x,y,"funcname",{args}},...} include:
    - radius: radius one edge arg {radius [, segments]}
    - belly: draw a belly between current and next point {distance [, segments]}
    - arc: arc where current point is center and previous/next points are limits {radius [, segments]}
--]]
------------------------------------------
local scad_polygon = [[
polygon(points=$COORDINATES$PATHS$CONVEXITY);
]]
function cad.polygon(args)
  if type(args) ~= "table" then
    error("polygon() requires a table argument with points")
  end

  if not args.points then
    error("polygon() requires points parameter")
  end

  local obj = cad._helpers.cad_obj()

  -- Process special functions in points if needed
  local t_points = geometry.polygon(args.points)
  local t_paths = args.paths
  local convexity = args.convexity

  -- create coordinates string
  local coor = "["
  for i, v in ipairs(t_points) do
    coor = coor .. "[" .. valOrName(v[1]) .. "," .. valOrName(v[2]) .. "],"
  end
  coor = string.sub(coor, 1, -2) .. "]"

  local paths = ""
  if t_paths then
    paths = ", paths=["
    for i, v in ipairs(t_paths) do
      paths = paths .. "[" .. table.concat(valOrName(v), ",") .. "],"
    end
    paths = string.sub(paths, 1, -2) .. "]"
  end

  local conv = ""
  if convexity then
    conv = ", convexity=" .. convexity
  end

  local t = {
    COORDINATES = coor,
    PATHS = paths,
    CONVEXITY = conv,
  }
  cad._helpers.update_content_t(obj, scad_polygon, t)
  return obj
end

--[[--------------------------------------------------------------------------------
  GEOMETRIC 2D OBJECTS
--]]
--------------------------------------------------------------------------------

--[[----------------------------------------
  function cad.rectround(args)

  draw rectangle with rounded edges
  args = {size = {width, height}, radius = r, center = true/false}
--]]
----------------------------------------
function cad.rectround(args)
  local size = args.size
  local width = size[1]
  local height = size[2]
  local radius = args.radius
  local isCenter = args.center or false

  local x = (isCenter and -width / 2) or 0
  local y = (isCenter and -height / 2) or 0

  local obj = cad._helpers
    .cad_obj()
    :add(
      cad.circle({ r = radius }):translate(radius, radius, 0),
      cad.circle({ r = radius }):translate(width - radius, radius, 0),
      cad.circle({ r = radius }):translate(width - radius, height - radius, 0),
      cad.circle({ r = radius }):translate(radius, height - radius, 0)
    )
    :translate(x, y)
    :hull()
  return obj
end

--[[----------------------------------------
  function cad.rectchamfer(args)

  draw rectangle with chamfered edges
  args = {size = {width, height}, chamfer = c, center = true/false}
--]]
----------------------------------------
function cad.rectchamfer(args)
  local size = args.size
  local width = size[1]
  local height = size[2]
  local chamfer = args.chamfer
  local isCenter = args.center or false

  local x = (isCenter and -width / 2) or 0
  local y = (isCenter and -height / 2) or 0

  -- make 45° angle edges
  local t_points = {
    { 0, chamfer },
    { chamfer, 0 },
    { width - chamfer, 0 },
    { width, chamfer },
    { width, height - chamfer },
    { width - chamfer, height },
    { chamfer, height },
    { 0, height - chamfer },
  }

  local obj = cad.polygon {
    points = t_points,
  }
  if x ~= 0 or y ~= 0 then
    obj:translate(x, y)
  end
  return obj
end

--[[------------------------------------------
  function cad.longhole(args)

  draws a long hole
  args = {points = {{x1, y1}, {x2, y2}}, r = radius}
--]]
------------------------------------------
local scad_longhole = [[
hull()
{
  translate([$X1,$Y1,0])
  circle(r = $RADIUS);
  translate([$X2,$Y2,0])
  circle(r = $RADIUS);
}
]]
function cad.longhole(args)
  local obj = cad._helpers.cad_obj()
  local points = args.points
  local x1, y1 = points[1][1], points[1][2]
  local x2, y2 = points[2][1], points[2][2]
  local radius = args.r

  local t = { X1 = x1, Y1 = y1, X2 = x2, Y2 = y2, RADIUS = radius }
  cad._helpers.update_content_t(obj, scad_longhole, t)
  return obj
end

--[[------------------------------------------
  function cad.longholerelative(args)

  draws a long hole using relative coordinates for the second point
  args = {point = {x, y}, delta = {dx, dy}, r = radius}
--]]
------------------------------------------
function cad.longholerelative(args)
  local point = args.point
  local delta = args.delta
  local x, y = point[1], point[2]
  local dx, dy = delta[1], delta[2]
  local radius = args.r

  return cad.longhole {
    points = { { x, y }, { x + dx, y + dy } },
    r = radius,
  }
end

--[[------------------------------------------------------------------------------------

  function cad.text(x ,y, z, text [, height [, font [, style [, h_align [, v_align] ] ] ] ])

  height = height in mm or 10 by default
  font = "Arial", "Courier New", "Liberation Sans", etc., default "Arial"
  style = depending on the font, usually "Bold","Italic", etc. default normal
  h_aling = horizontal alignment, Possible values are "left", "center" and "right". Default is "left".
  v_valign = vertical alignment for the text. Possible values are "top", "center", "baseline" and "bottom". Default is "baseline".

--]]
------------------------------------------------------------------------------------

local scad_text = [[
translate([$X,$Y,$Z])
text("$TEXT", size=$H, font="$FONT", halign="$HALIGN", valign="$VALIGN");
]]
function cad.text(x, y, z, text, height, font, style, h_align, v_align)
  local height = height or 10
  local font = font or "Arial"
  local style = style and (":" .. style) or ""
  local h_align = h_align or "left"
  local v_align = v_align or "baseline"
  local t = {
    X = x,
    Y = y,
    Z = z,
    TEXT = tostring(text),
    H = height,
    FONT = font .. style,
    HALIGN = h_align,
    VALIGN = v_align,
  }
  local obj = cad._helpers.cad_obj()
  cad._helpers.update_content_t(obj, scad_text, t)
  return obj
end

--[[------------------------------------------------------------------------------------

  function cad.text3d(x ,y, z, text [, height [, depth [, font [, style [, h_align [, v_align] ] ] ] ] ])

  height = height in mm or 1 by default
  depth = 1 by default
  font = "Arial", "Courier New", "Liberation Sans", etc., default "Arial"
  style = depending on the font, usually "Bold","Italic", etc. default normal
  h_aling = horizontal alignment, Possible values are "left", "center" and "right". Default is "left".
  v_valign = vertical alignment for the text. Possible values are "top", "center", "baseline" and "bottom". Default is "baseline".

--]]
------------------------------------------------------------------------------------
function cad.text3d(x, y, z, text, height, depth, font, style, h_align, v_align)
  local obj = cad
    .text(0, 0, 0, text, height, font, style, h_align, v_align)
    :linearextrude(depth or 1)
    :translate(x, y, z)
  return obj
end
