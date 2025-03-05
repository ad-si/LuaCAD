--[[--================================================================================
  Lua CAD Main Module
  This module loads all the necessary CAD modules
--]]
--================================================================================

-- Load all CAD modules
require("lib_cad_core")

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
cad.let = function(var, expr)
  return expr
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
