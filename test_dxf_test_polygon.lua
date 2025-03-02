
require "lib_dxf"

local dxf_obj = dxf()

local dist = 20

local points = {
	{0,0},
	{dist, dist},
	{2 * dist, 0},
	{3 * dist, dist},
	{4 * dist, 0},
	{5 * dist, dist},
	{6 * dist, 0},
	{6 * dist, 3 * dist},
	{4 * dist, 3 * dist},
	{4 * dist, 2 * dist},
	{3 * dist, 2 * dist},
	{3 * dist, 3 * dist},
	{2 * dist, 3 * dist},
	{2 * dist, 2 * dist},
	{0,2 * dist},
	{0,0},
}
dxf_obj:polygon(0,0, points)


local points = {
	{0,0},
	{dist, dist,  "radius", {-5}},
	{2 * dist, 0, "radius", {3}},
	{3 * dist, dist},
	{4 * dist, 0},
	{5 * dist, dist},
	{6 * dist, 0, "belly", {10}},
	{6 * dist, 3 * dist},
	{4 * dist, 3 * dist,  "belly", {-4}},
	{4 * dist, 2 * dist},
	{3 * dist, 2 * dist},
	{3 * dist, 3 * dist,  "arc", {-4}},
	{2 * dist, 3 * dist,  "arc", {4}},
	{2 * dist, 2 * dist,  "radius", {-5}},
	{0,2 * dist},
	{0,0},
}
dxf_obj:polygon(8 * dist, 0, points)


dxf_obj:save("temp/dxf_test_polygon.dxf")
