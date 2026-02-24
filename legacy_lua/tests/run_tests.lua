local luaunit = require("luaunit")

isTesting = true

-- Load all test files
require("tests/test_circle")
require("tests/test_cube")
require("tests/test_diabolo_cylindric")
require("tests/test_dxf_test_arc")
require("tests/test_dxf_test_polygon")
require("tests/test_export_wrl")
require("tests/test_geometry")
require("tests/test_longhole")
require("tests/test_minkowski")
require("tests/test_multmatrix")
require("tests/test_projection")
require("tests/test_pyramid_cut")
require("tests/test_rect_variants")
require("tests/test_rotate_extrude")
require("tests/test_sign")
require("tests/test_simple")
require("tests/test_sphere")
require("tests/test_square")
require("tests/test_text")
require("tests/test_vector_cross")

os.exit(luaunit.LuaUnit.run())
