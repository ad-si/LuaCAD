require("lib_vector")

--[[----------------------------------------------------------------------------
  GEOMETRY TABLE
--]]

geometry = {}

-- Variables
geometry.default_segments = 120

--[[------------------------------------------
  function geometry.gettrianglefromsides(side_a, side_b, side_c)

  calc triangle when 3 sides are given

  http://en.wikipedia.org/wiki/Triangle#Sine.2C_cosine_and_tangent_rules
  alpha = arccos((b+c-a)/2bc)

  c = bottom side, a(right), b(left)

  returns A,B,C, alpha,beta,gamma
--]]
------------------------------------------
geometry.gettrianglefromsides = function(side_a, side_b, side_c)
  local alpha = math.acos(
    (
      ((side_b * side_b) + (side_c * side_c) - (side_a * side_a))
      / (2 * side_b * side_c)
    )
  )

  -- calc point C
  y = math.round(math.sin(alpha) * side_b)
  x = math.round(math.cos(alpha) * side_b)

  return { 0, 0 }, { side_c, 0 }, { x, y }, math.deg(alpha)
end

-- function to draw pyramid
geometry.getpyramidcircumcenter = function(length, height)
  -- calc circumcenter point
  -- calc side len half
  local len_2 = length / 2
  local l = math.sqrt((len_2 * len_2) + (height * height))
  --local l_2 = l/2
  -- calc angle
  --local alpha = math.asin(height/l)
  local d = (l * l) / (2 * height)
  -- return hight above ground
  return height - d
end

geometry.getpyramidinnercircumcenter = function(length, height)
  -- calc innercircumcenter point
  local beta = math.atan((2 * height) / length)
  local alpha = beta / 2
  local d = (math.tan(alpha) * length) / 2
  return d
end

-- x1,y1 and x2,y2 are the vector, width is the hight from the middle of the vector to radius
geometry.getradiusfromcirclesegment = function(x1, y1, x2, y2, width)
  local s_vec = vector(x1 - x2, y1 - y2)
  local s_vec_len = #s_vec
  -- calc radius
  local radius = ((4 * (width * width)) + (s_vec_len * s_vec_len)) / (8 * width)

  -- s_vec_normal to the right as unit
  local s_vec_norm = s_vec:normal(-1):unit()

  -- calc radius point
  local rp = vector(x2, y2) + (s_vec * 0.5)
  local vlen = radius - width
  local rp = rp + (s_vec_norm * vlen)

  return rp, radius
end

--[[------------------------------------------
  function geometry.getcirclesegment(x1, y1, x2, y2, distance [, circle_seg])

  get points following a circle between points 1 and 2 with distance as the maximum belly

  returns a table with points
--]]
------------------------------------------
geometry.getcirclesegment = function(x1, y1, x2, y2, dist, circle_seg)
  if dist < 0 then
    x1, y1, x2, y2 = x2, y2, x1, y1
  end
  local circle_seg = circle_seg or geometry.default_segments
  local angle_seg = 360 / circle_seg
  local rp, radius =
    geometry.getradiusfromcirclesegment(x1, y1, x2, y2, math.abs(dist))

  local v1 = vector(x1, y1) - rp
  local v2 = vector(x2, y2) - rp

  -- get absolute angle between
  local angle = math.abs(v1:angle(v2))

  local num = math.floor(angle / angle_seg)
  local rest = (angle - (num * angle_seg)) / 2

  local t_points = {}

  local vrot = v1:rot(rest)
  table.insert(t_points, vrot + rp)
  for i = 1, num do
    vrot = vrot:rot(angle_seg)
    table.insert(t_points, vrot + rp)
  end

  if dist < 0 then
    local t_rev = {}
    for i, v in ipairs(t_points) do
      table.insert(t_rev, 1, v)
    end
    t_points = t_rev
  end
  return t_points
end

--[[------------------------------------------
  function geometry.arc(x, y, radius, start_angle, end_angle [, circle_seg])

  start_angle: 0 = right, 90 = top, 180 = left
  returns a table with points
--]]
------------------------------------------
geometry.arc = function(x, y, radius, start_angle, end_angle, circle_seg)
  local circle_seg = circle_seg or geometry.default_segments
  local angle_seg = 360 / circle_seg
  local start_angle = math.fmod(start_angle, 360)
  local end_angle = math.fmod(end_angle, 360)

  -- calc full angle
  local full_angle = math.abs(end_angle - start_angle)
  -- calc segments
  local num = math.floor(full_angle / angle_seg)
  -- calc rest
  local rest = (full_angle - (num * angle_seg)) / 2

  -- get vector
  local v1 = vector(radius, 0):rot(start_angle)
  local center = vector(x, y)

  local t_points = {}
  table.insert(t_points, v1 + center)
  v1 = v1:rot(rest)
  table.insert(t_points, v1 + center)
  for i = 1, num do
    v1 = v1:rot(angle_seg)
    table.insert(t_points, v1 + center)
  end
  v1 = v1:rot(rest)
  table.insert(t_points, v1 + center)

  return t_points
end

--[[------------------------------------------
  function geometry.arcvec(x1,y1, x2,y2, x3,y3, radius [, circle_seg])

  get points of radius between vec1(2->1) and vec2(2->3), center is x2,y2
  returns a table with points
--]]
------------------------------------------
geometry.arcvec = function(x1, y1, x2, y2, x3, y3, radius, circle_seg)
  local circle_seg = circle_seg or geometry.default_segments
  local angle_seg = 360 / circle_seg

  local v1 = vector(x1 - x2, y1 - y2)
  local v2 = vector(x3 - x2, y3 - y2)

  -- calc full angle
  local full_angle = v1:angle(v2)
  if full_angle < 0 then
    full_angle = 360 + full_angle
  end
  if radius < 0 then
    full_angle = full_angle - 360
    angle_seg = -angle_seg
  end

  -- calc segments
  local num = math.floor(full_angle / angle_seg)
  -- calc rest
  local rest = (full_angle - (num * angle_seg)) / 2

  -- get vector
  local v_rot = v1:unit() * math.abs(radius)
  local v2 = vector(x2, y2)

  local t_points = {}
  table.insert(t_points, v_rot + v2)

  v_rot = v_rot:rot(rest)
  table.insert(t_points, v_rot + v2)
  for i = 1, num do
    v_rot = v_rot:rot(angle_seg)
    table.insert(t_points, v_rot + v2)
  end
  v_rot = v_rot:rot(rest)
  table.insert(t_points, v_rot + v2)

  return t_points
end

--[[------------------------------------------
  function geometry.getroundcenter(x1,y1, x2,y2, x3,y3, radius, outside)

  get rounding center of three points (two vectors with one center points)
  points have to be in counter clock direction
  A = center
  B = first circler connection
  D = last circle connection
  gamma = the angle that the circle is drawn in radians
--]]
------------------------------------------
geometry.getroundcenter = function(x1, y1, x2, y2, x3, y3, radius, outside)
  local outside = outside or false
  -- calc vecs
  local vec_1 = vector(x1 - x2, y1 - y2)
  local vec_2 = vector(x3 - x2, y3 - y2)

  local gamma = math.acos(vec_1:scalar(vec_2) / (#vec_1 * #vec_2))
  -- get oposite
  gamma = math.pi - gamma
  local gamma_2 = gamma / 2
  local alpha = gamma_2

  -- triangle A,B,C, a,b,c
  local c = math.abs(radius)
  local a = math.tan(alpha) * c

  local vec_a = vec_1:unit() * a
  local left_right = -1
  if outside == true then
    left_right = 1
  end
  local vec_c = vec_1:unit():normal(left_right) * c
  --print(vec_1, vec_2, vec_a, vec_c)

  -- get radius point
  local A = vector(x2, y2) + vec_a + vec_c
  local B = vector(x2, y2) + vec_a
  local D = vector(x2, y2) + (vec_2:unit() * a)
  return A, B, D, gamma
end

--[[------------------------------------------
  function geometry.roundcorner(x1,y1, x2,y2, x3,y3, radius, segments)

  returns points to use instead of the middle point e.g. {x,y},{x,y}
--]]
------------------------------------------
geometry.roundcorner = function(x1, y1, x2, y2, x3, y3, radius, segments)
  local segments = segments or geometry.default_segments
  local seg_rad = (2 * math.pi) / segments

  local outside = false
  if radius < 0 then
    outside = true
  end
  local A, B, D, gamma =
    geometry.getroundcenter(x1, y1, x2, y2, x3, y3, radius, outside)

  local vec_ab = B - A
  local vec_ad = D - A

  local segs = math.floor(gamma / seg_rad)
  if segs == 0 then
    --local p = A + vec_ab:rot(gamma/2)
    return { B, D }
  end

  local rest = (gamma - (seg_rad * segs)) / 2
  if rest < 1 then
    rest = 0
  end

  local rot = 0
  -- calc new points
  local t_points = {}
  if rest > 0 then
    if radius < 0 then
      rot = rot - rest
    else
      rot = rot + rest
    end
    local p = A + vec_ab:rot(math.deg(rot))
    table.insert(t_points, p)
  end
  for i = 1, segs do
    if radius < 0 then
      rot = rot - seg_rad
    else
      rot = rot + seg_rad
    end
    local p = A + vec_ab:rot(math.deg(rot))
    table.insert(t_points, p)
  end
  --table.insert(t_points, D)
  return t_points
end

-- Geometric Objects

-- returns the coordinates of a tetrahedron with equal sides
geometry.tetrahedron = function(side)
  local A = { 0, 0, 0 }
  local B = { side, 0, 0 }
  -- calc C
  local h = side * math.sqrt(0.5)
  local C = { side / 2, h, 0 }
  -- middle point, center point gravity point
  local M = { side / 2, h / 3 }
  local h_D = side * math.sqrt(1 / 3)
  local D = { side / 2, h / 3, h_D }

  return A, B, C, D
end

--[[------------------------------------------
  function geometry.polygon(x, y, t_points)

  points have to be in counter clockwise direction
  draw a polygon from given points {{x,y [,"<funcname>",{args}]},{x,y},...,{x,y}}
  note angles are the outer angles measured
  special functions:
    radius: radius one edge arg {radius [, segments]}, replaces current point with radius
    belly: draw a belly between current and next point, the belly function follows a circle {distance [, segments]}
    arc: draws an arc where current point is the center and previous point and next point are the limits {radius [, segments]}

  Note: if using special function t_paths will not work
--]]
------------------------------------------
local t_fname = {
  ["radius"] = function(pre, cur, nex, t_param)
    return geometry.roundcorner(
      pre[1],
      pre[2],
      cur[1],
      cur[2],
      nex[1],
      nex[2],
      t_param[1],
      t_param[2]
    )
  end,
  ["belly"] = function(pre, cur, nex, t_param)
    local tp = geometry.getcirclesegment(
      cur[1],
      cur[2],
      nex[1],
      nex[2],
      t_param[1],
      t_param[2]
    )
    table.insert(tp, 1, cur)
    return tp
  end,
  ["arc"] = function(pre, cur, nex, t_param)
    return geometry.arcvec(
      pre[1],
      pre[2],
      cur[1],
      cur[2],
      nex[1],
      nex[2],
      t_param[1],
      t_param[2]
    )
  end,
}
function geometry.polygon(t_points)
  local t_p = {}
  for i, v in ipairs(t_points) do
    -- previus
    local prev = 0
    local nex = 0
    if i == 1 then
      prev = t_points[#t_points]
    else
      prev = t_points[i - 1]
    end
    if i == #t_points then
      nex = t_points[1]
    else
      nex = t_points[i + 1]
    end
    local f_name = t_points[i][3]
    if f_name and t_fname[f_name] then
      local p = t_fname[f_name](prev, v, nex, t_points[i][4])
      for i, v in ipairs(p) do
        table.insert(t_p, v)
      end
    else
      table.insert(t_p, { v[1], v[2] })
    end
  end
  return t_p
end
