-- BOSL2 (Belfry OpenSCAD Library v2) functions demo
-- All BOSL2 functions are available under the `bosl` namespace.
-- When exported to .scad, the necessary include directives are
-- added automatically.

-- A cuboid with rounded edges
local body = bosl.cuboid { {30, 20, 10}, rounding = 2, center = true }

-- A threaded rod (from threading.scad, auto-included)
local rod = bosl.threaded_rod { d = 10, l = 25, pitch = 2 }

-- A spur gear (from gears.scad, auto-included)
local my_gear = bosl.spur_gear {
  circ_pitch = 5,
  teeth = 20,
  thickness = 5,
  shaft_diam = 5,
}

-- A NEMA 17 stepper motor model (from nema_steppers.scad, auto-included)
local _motor = bosl.nema17_stepper { h = 34, shaft = 5 }

-- BOSL2 constants are also available
local _up = bosl.UP       -- {0, 0, 1}
local _phi = bosl.PHI     -- golden ratio

-- Combine objects (BOSL2 objects support all normal CSG operations)
render(body:translate(0, 0, 0))
render(rod:translate(40, 0, 0))
render(my_gear:translate(0, 40, 0))
