require("luacad")

-- Creating some objects using LuaCAD
my_cube = cube { size = { 1, 2, 3 } }
my_sphere = sphere({ r = 2 }):translate(5, 0, 0)

-- Using scad() to insert custom OpenSCAD code directly
custom_scad = scad([[
module fancy_cylinder() {
  cylinder(h=10, r1=3, r2=1, center=true, $fn=50);
}
fancy_cylinder();
]])

-- Using scad() for simple single-line commands
another_scad = scad("cylinder(h=5, r=2, center=true);"):translate(0, 10, 0)

-- Combining all objects
model = my_cube + my_sphere + custom_scad + another_scad

-- Export the final model
model:export("literal-openscad.scad")
