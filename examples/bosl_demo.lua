-- BOSL (Belfry OpenSCAD Library) functions demo
-- All BOSL functions are available under the `bosl` namespace.
-- When exported to .scad, the necessary include/use directives are
-- added automatically.

-- A cuboid with fillet edges
local body = bosl.cuboid { {30, 20, 10}, fillet = 2, center = true }

-- A threaded rod
local rod = bosl.threaded_rod { d = 10, l = 25, pitch = 2 }

-- An involute gear
local my_gear = bosl.gear {
  mm_per_tooth = 5,
  number_of_teeth = 20,
  thickness = 5,
  hole_diameter = 5,
}

-- A NEMA 17 stepper motor model
local motor = bosl.nema17_stepper { h = 34, shaft = 5 }

-- BOSL constants are also available
local up = bosl.V_UP     -- {0, 0, 1}
local phi = bosl.PHI     -- golden ratio

-- Combine objects (BOSL objects support all normal CSG operations)
render(body:translate(0, 0, 0))
render(rod:translate(40, 0, 0))
render(my_gear:translate(0, 40, 0))
