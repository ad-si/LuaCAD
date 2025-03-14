--// create stl or dxf file

require("src.export.dxf")

-- output file
local dxf_file = "template.dxf"

-- create dfx object
local part = dxf()

-- draw rect
local w = 100
local h = 50

part:rect(0, 0, w, h)

-- dxf add info
part:addlayer("INFO", 3)
part:text(dxf_file, (w / 2), h / 2, 8, 0, _, _, "INFO")
part:rect(w + 10, 0, w, h, "INFO")

part:addlayer("BLUE", 5)
part:rect(0, h + 10, w, h, "BLUE")

part:save(dxf_file)
