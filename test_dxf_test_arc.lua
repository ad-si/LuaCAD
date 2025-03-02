
require "lib_dxf"

local arc = dxf()

arc:arc(10,10, 50, 0, 45)

arc:arc(100,10, 30, -45, 90)

arc:save("temp/dxf_text_arc.dxf")
