-----------------
-- vector library
-----------------
--[[------------------------------------------
  a vector consists of 2 or 3 points, x,y and z
  v = {x,y,z}

  vector is 2D, limited 3D support
--]]
------------------------------------------

local vec_meta = {}

function vector(x, y, z)
  local v = {}
  if type(x) == "table" then
    v[1] = x[1]
    v[2] = x[2]
    v[3] = x[3]
  else
    v = { x, y, z }
  end
  return setmetatable(v, vec_meta)
end
vec = vector

-- functions
function vec_meta.__add(a, b)
  if a[3] then
    return vector(a[1] + b[1], a[2] + b[2], a[3] + b[3])
  end
  return vector(a[1] + b[1], a[2] + b[2])
end

function vec_meta.__sub(a, b)
  if a[3] then
    return vector(a[1] - b[1], a[2] - b[2], a[3] - b[3])
  end
  return vector(a[1] - b[1], a[2] - b[2])
end

-- two vectors does the scalar
function vec_meta.__mul(a, b)
  if type(a) == "table" and type(b) == "table" then
    return a.scalar(b)
  end
  if type(a) == "table" then
    a, b = b, a
  end
  if b[3] then
    return vector(a * b[1], a * b[2], a * b[3])
  end
  return vector(a * b[1], a * b[2])
end

-- -(vec)
function vec_meta.__unm(a)
  if a[3] then
    return vector(-a[1], -a[2], -a[3])
  end
  return vector(-a[1], -a[2])
end

function vec_meta.__len(a)
  return a:len()
end

function vec_meta.__tostring(a)
  if a[3] then
    return "vector(x: "
      .. a[1]
      .. ", y: "
      .. a[2]
      .. ", z: "
      .. a[3]
      .. "), #: "
      .. #a
  end
  return "vector(x: " .. a[1] .. ", y: " .. a[2] .. "), #: " .. #a
end

-- __index functions
vec_meta.__index = {}

vec_meta.__index.getx = function(a)
  return a[1]
end

vec_meta.__index.gety = function(a)
  return a[2]
end

vec_meta.__index.getz = function(a)
  return a[3]
end

vec_meta.__index.len = function(a)
  if a[3] then
    return math.sqrt((a[1] ^ 2) + (a[2] ^ 2) + (a[3] ^ 2))
  end
  return math.sqrt((a[1] ^ 2) + (a[2] ^ 2))
end

vec_meta.__index.unit = function(a)
  return (1 / #a) * a
end

-- normal vector, default direction is left (1), if direction = -1 then normal is right of vec
vec_meta.__index.normal = function(a, direction)
  local direction = direction or 1
  return vector(-direction * a[2], direction * a[1])
end

vec_meta.__index.scalar = function(a, b)
  return (a[1] * b[1] + a[2] * b[2])
end

-- cross product, 2D if z is bigger 0 then b is left from a else otherwise
vec_meta.__index.cross = function(a, b)
  if type(b) ~= "table" then
    return nil
  end

  -- Both vectors must have the same dimension (either 2D or 3D)
  local a_len = a[3] and 3 or 2
  local b_len = b[3] and 3 or 2

  if a_len ~= b_len then
    return nil
  end

  -- 3D cross product
  if a_len == 3 and b_len == 3 then
    local x = a[2] * b[3] - a[3] * b[2]
    local y = a[3] * b[1] - a[1] * b[3]
    local z = a[1] * b[2] - a[2] * b[1]
    return vector(x, y, z)
  end

  -- 2D cross product (returns scalar)
  if a_len == 2 and b_len == 2 then
    return a[1] * b[2] - a[2] * b[1]
  end

  return nil
end

--[[------------------------------------------
  function <a>:rot(phi [, axis])

  rotates a by angle <phi>, phi is in degrees
  axis is optional and defines the rotate axis by default = "z", other values, "x", "y"
  returns new rotated vector
  note only 2d
--]]
------------------------------------------
vec_meta.__index.rot = function(a, phi, axis)
  local axis = axis or "z"
  local phi = math.rad(phi)
  if axis == "z" then
    local x = math.cos(phi) * a[1] - math.sin(phi) * a[2]
    local y = math.sin(phi) * a[1] + math.cos(phi) * a[2]
    return vector(x, y, a[3])
  end
  if axis == "y" then
    local x = math.cos(phi) * a[1] - math.sin(phi) * a[3]
    local z = math.sin(phi) * a[1] + math.cos(phi) * a[3]
    return vector(x, a[2], z)
  end
  if axis == "x" then
    local y = math.cos(phi) * a[2] - math.sin(phi) * a[3]
    local z = math.sin(phi) * a[2] + math.cos(phi) * a[3]
    return vector(a[1], y, z)
  end
end

--[[------------------------------------------
  function <a>:angle(<b>)

  calculates the angle between 2 vectors, it always calculates the smallest angle
  if angle is positive then <b> is left of <a> if angle is negative then <b> is right of <a>
  returns angle in degrees
--]]
------------------------------------------
vec_meta.__index.angle = function(a, b)
  local scalar = a:scalar(b)
  local cos_alpha = scalar / (#a * #b)
  local angle = math.deg(math.acos(cos_alpha))
  local cross_product_z = a:cross(b):getz()
  if cross_product_z < 0 then
    return -angle
  end
  return angle
end
