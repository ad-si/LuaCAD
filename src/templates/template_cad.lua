--// create stl or dxf file

require("src.core.cad")

-- output file
local cad_file = "template_design.stl"

-- create cad object
local part = cad()

-- part:setcirlcesegments(36)

--part:setcolor("red")

part:export(cad_file)
