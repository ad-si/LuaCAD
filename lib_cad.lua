--[[--================================================================================
  Lua CAD Main Module
  This module loads all the necessary CAD modules
--]]
--================================================================================

-- Load all CAD modules
require("lib_cad_core")

-- Global variable for storing OpenSCAD variable assignments
cad.scad_variables = ""

-- Re-export math functions for OpenSCAD compatibility
cad.abs = math.abs
cad.sin = math.sin
cad.cos = math.cos
cad.tan = math.tan
cad.acos = math.acos
cad.asin = math.asin
cad.atan = math.atan
cad.atan2 = math.atan2
cad.floor = math.floor
cad.round = geometry.round
cad.ceil = math.ceil
cad.ln = math.log
cad.len = function(a)
  return #a
end

local VarObj = {}

function VarObj:new(o)
  o = o or {}
  o.tableType = "var"
  setmetatable(o, self)
  self.__index = self
  return o
end

function VarObj:add(v)
  if type(v) == "number" then
    return VarObj:new {
      name = "(" .. self.name .. " + " .. v .. ")",
      value = self.value + v,
    }
  end
  return VarObj:new {
    name = "(" .. self.name .. " + " .. v.name .. ")",
    value = self.value + v.value,
  }
end
VarObj.__add = VarObj.add

function VarObj:sub(v)
  if type(v) == "number" then
    return VarObj:new {
      name = "(" .. self.name .. " - " .. v .. ")",
      value = self.value - v,
    }
  end
  return VarObj:new {
    name = "(" .. self.name .. " - " .. v.name .. ")",
    value = self.value - v.value,
  }
end
VarObj.__sub = VarObj.sub

function VarObj:mul(v)
  if type(v) == "number" then
    return VarObj:new {
      name = "(" .. self.name .. " * " .. v .. ")",
      value = self.value * v,
    }
  end
  return VarObj:new {
    name = "(" .. self.name .. " * " .. v.name .. ")",
    value = self.value * v.value,
  }
end
VarObj.__mul = VarObj.mul

function VarObj:div(v)
  if type(v) == "number" then
    return VarObj:new {
      name = "(" .. self.name .. " / " .. v .. ")",
      value = self.value / v,
    }
  end
  return VarObj:new {
    name = "(" .. self.name .. " / " .. v.name .. ")",
    value = self.value / v.value,
  }
end
VarObj.__div = VarObj.div

function VarObj:pow(v)
  if type(v) == "number" then
    return VarObj:new {
      name = "(" .. self.name .. " ^ " .. v .. ")",
      value = self.value ^ v,
    }
  end
  return VarObj:new {
    name = "(" .. self.name .. " ^ " .. v.name .. ")",
    value = self.value ^ v.value,
  }
end
VarObj.__pow = VarObj.pow

function VarObj:unm(v)
  v.name = "-" .. v.name
  v.value = -v.value
  return v
end
VarObj.__unm = VarObj.unm

-- Comparison operators
function VarObj:lt(v)
  if type(v) == "number" then
    return VarObj:new {
      name = "(" .. self.name .. " < " .. v .. ")",
      value = self.value < v,
    }
  end
  return VarObj:new {
    name = "(" .. self.name .. " < " .. v.name .. ")",
    value = self.value < v.value,
  }
end
VarObj.__lt = VarObj.lt

function VarObj:le(v)
  if type(v) == "number" then
    return VarObj:new {
      name = "(" .. self.name .. " <= " .. v .. ")",
      value = self.value <= v,
    }
  end
  return VarObj:new {
    name = "(" .. self.name .. " <= " .. v.name .. ")",
    value = self.value <= v.value,
  }
end
VarObj.__le = VarObj.le

function VarObj:gt(v)
  if type(v) == "number" then
    return VarObj:new {
      name = "(" .. self.name .. " > " .. v .. ")",
      value = self.value > v,
    }
  end
  return VarObj:new {
    name = "(" .. self.name .. " > " .. v.name .. ")",
    value = self.value > v.value,
  }
end
VarObj.__gt = VarObj.gt

function VarObj:ge(v)
  if type(v) == "number" then
    return VarObj:new {
      name = "(" .. self.name .. " >= " .. v .. ")",
      value = self.value >= v,
    }
  end
  return VarObj:new {
    name = "(" .. self.name .. " >= " .. v.name .. ")",
    value = self.value >= v.value,
  }
end
VarObj.__ge = VarObj.ge

function VarObj:eq(v)
  if type(v) == "number" then
    return VarObj:new {
      name = "(" .. self.name .. " == " .. v .. ")",
      value = self.value == v,
    }
  end
  return VarObj:new {
    name = "(" .. self.name .. " == " .. v.name .. ")",
    value = self.value == v.value,
  }
end
VarObj.__eq = VarObj.eq

function VarObj:ne(v)
  if type(v) == "number" then
    return VarObj:new {
      name = "(" .. self.name .. " ~= " .. v .. ")",
      value = self.value ~= v,
    }
  end
  return VarObj:new {
    name = "(" .. self.name .. " ~= " .. v.name .. ")",
    value = self.value ~= v.value,
  }
end
VarObj.__ne = VarObj.ne

function VarObj:__tostring()
  return self.name
end

function cad.var(var, expr)
  if type(var) ~= "string" then
    error(
      "cad.var: First parameter (variable name) must be a string, got "
        .. type(var)
    )
  end

  local varNorm = var:gsub(" ", "_")

  -- Validate variable name format (simple check for valid identifier)
  if not varNorm:match("^[%a_][%w_]*$") then
    error("cad.var: Invalid variable name format: '" .. varNorm .. "'")
  end

  -- Store variable assignment in global scad_variables
  cad.scad_variables = cad.scad_variables
    .. varNorm
    .. " = "
    .. tostring(expr)
    .. ";\n"

  return VarObj:new { name = varNorm, value = expr }
end

cad.log = math.log
cad.pow = math.pow
cad.sqrt = math.sqrt
cad.exp = math.exp
cad.rands = function(min_value, max_value, count, seed)
  local result = {}
  if seed then
    math.randomseed(seed)
  end
  for i = 1, count do
    table.insert(result, min_value + math.random() * (max_value - min_value))
  end
  return result
end
cad.min = math.min
cad.max = math.max
cad.norm = function(a)
  return #a
end
cad.cross = function(a, b)
  return a:cross(b)
end

-- OpenSCAD's "show only" modifier (equivalent to ! in OpenSCAD)
function cad.s(obj)
  local result = obj:clone()
  result.showOnly = true
  return result
end
