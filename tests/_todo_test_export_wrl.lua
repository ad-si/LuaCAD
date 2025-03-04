--// create stl or dxf file

require("lib_cad")

-- output file
local cad_file = "test_export_wrl"

-- create cad object
local box = cad.cube { size = { 10, 20, 30 } }
-- export blue box
box:export(cad_file .. "_box.wrl", { 0, 0, 1 })
-- export as stl
box:export(cad_file .. "_box.stl")

local cyl = cad.cylinder(0, 0, 0, 50, 10, 5)
-- export as stl
cyl:export(cad_file .. "_cylinder.stl")

-- combine using cad.io
cad.io.stltowrl(
  cad_file .. ".wrl",
  { cad_file .. "_box.stl", cad_file .. "_cylinder.stl" },
  { { 1.0, 0, 0 }, { 0, 1.0, 0 } }
)
