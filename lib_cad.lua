--[[--================================================================================

	Lua CAD fuctions
	provides simple useful functions to create stl files
	built on usage of openscad

	updated version this one creates a scad file which then can be processed by open scad
	Creates a cad obj that can be used, and imported by other cad objects

	cad.<function> returns a new cad object
	<cad>:<function> always changes the current obj
--]]--================================================================================

--[[--------------------------------------------------------------------------------
	Author: Michael Lutz
	Licensed under the same terms as Lua itself
--]]--------------------------------------------------------------------------------

--[[--------------------------------------------------------------------------------
	Settings table
--]]--------------------------------------------------------------------------------
--[=[
local openscad_settings = {
	executable = [["C:\Program Files\OpenSCAD\openscad.exe"]],
	scadfile = [[D:\Temp\lua_openscad_tmp.scad]],
	include= ""
}
return openscad_settings
--]=]

require "lib_geometry" -- includes lib_vector

--[[--------------------------------------------------------------------------------
	GLOBAL CAD TABLE
--]]--------------------------------------------------------------------------------
cad = {}

-- OpenSCAD settings
cad.settings = require "lib_cad_settings"

--[[--------------------------------------------------------------------------------
	LOCAL OBJ HELPER FUNCTIONS
--]]--------------------------------------------------------------------------------
local default_segments = cad.settings.default_segments
local executable = cad.settings.executable
local scadfile = cad.settings.scadfile

local function getExtension(path)
	return "."..path:lower():match("%.([^%.]+)$")
end

local function intend_content(this, count)
	this.intend_num = this.intend_num + count
end

local function update_content(this, str)
	local s = string.rep("\t", this.intend_num)
	str = s..string.gsub(str, "\n(.)", function(c) return "\n"..s..c end)
	this.scad_content = this.scad_content..str
end

local function update_content_t(obj, scad_func, t)
	local _scad_content = string.gsub(scad_func, "%$(%w+)", t)
	update_content(obj, _scad_content)
end

-- add aditonal presets of colors here
local color_rgb = {
	["white"] = {1.0,1.0,1.0},
	["black"] = {0,0,0},
	["red"] = {1.0,0,0},
	["green"] = {0,1.0,0},
	["blue"] = {0,0,1.0},
	["cyan"] = {0,1.0,1.0},
	["magenta"] = {1.0,0,1.0},
	["yellow"] = {1.0,1.0,0},
}

local function export_scad(obj, file)
	-- create native SCAD file
	local fscad = io.open(scadfile, "w")
	if not fscad then
		error("Failed to write to scad file: "..fscad)
	end
	fscad:write(cad.settings.include.."\n")
	fscad:write("$fn = "..obj.segments..";\n\n")
	fscad:write(obj.scad_content)
	fscad:close()

	if getExtension(file) == ".scad" then
		os.rename(scadfile, file)
	else
		-- Execute OpenSCAD to create the final output file
		os.execute(executable.." -o "..file.." "..scadfile)
	end
end

--[[--------------------------------------------------------------------------------
	GLOBAL CAD DIRECT FUNCTIONS, returning new obj
--]]--------------------------------------------------------------------------------

--[[----------------------------------------
	function cad()

	returning new cad object
--]]----------------------------------------
local cad_meta = {}
local cad_obj = function()
	local obj = setmetatable({}, cad_meta)
	obj:init()
	return obj
end
setmetatable(cad, {__call = cad_obj })

--[[----------------------------------------
	function cad.setdefaultcirclefragments(num)

	optional $fn and $fs can be set (http://en.wikibooks.org/wiki/OpenSCAD_User_Manual/Other_Language_Features#Special_variables)
--]]----------------------------------------
cad.setdefaultcirclefragments = function(fn)
	default_segments = fn
end

--[[--------------------------------------------------------------------------------
	CAD CREATING Functions
	returning new obj
--]]--------------------------------------------------------------------------------

--[[--------------------------------------------------------------------------------
	2D OBJECTS

	Only give x and y, z is obsolete since linear extrude moves the item back to zero z.
--]]--------------------------------------------------------------------------------

--[[----------------------------------------
	function cad.rect(x, y, width, height [, center])

	draw rectangle width given width and height
--]]----------------------------------------
local scad_rect =[[
// rect
translate([$X,$Y,0])
square([$WIDTH,$HEIGHT],center=$CENTER]);
]]
function cad.rect(x,y, width,height, center)
	local obj = cad_obj()
	local center = center or false
	local t = {X=x, Y=y, WIDTH=width, HEIGHT=height, CENTER=center}
	update_content_t(obj, scad_rect, t)
	return obj
end
cad.square = rect

--[[------------------------------------------
	function cad.circle(x, y, radius)

	draw circle
--]]------------------------------------------
local scad_circle =[[
// circle
translate([$X,$Y,0])
circle(r = $RADIUS);
]]
function cad.circle(x,y, radius)
	local obj = cad_obj()
	local t = {X=x, Y=y, RADIUS=radius}
	update_content_t(obj, scad_circle, t)
	return obj
end

--[[------------------------------------------
	function cad.polygon(x, y, t_points [, t_paths])

	points have to be in counter clockwise direction
	draw a polygon from given points {{x,y [,"<funcname>",{args}]},{x,y},...,{x,y}}
	note angles are the outer angles measured
	special functions:
		radius: radius one edge arg {radius [, segments]}, replaces current point with radius
		belly: draw a belly between current and next point, the belly function follows a circle {distance [, segments]}
		arc: draws an arc where current point is the center and previous point and next point are the limits {radius [, segments]}

	Note: if using special function t_paths will not work
--]]------------------------------------------
local scad_polygon =[[
translate([$X,$Y,0])
polygon(points=$COORDINATES);
]]
local polygon_raw = function(x,y, t_points, t_paths)
	local obj = cad_obj()
	-- create coordinates string
	local coor = "["
	for i,v in ipairs(t_points) do
		coor = coor.."["..v[1]..","..v[2].."],"
	end
	coor = string.sub(coor,1,-2).."]"
	if t_paths then
		coor = coor..",paths=["
		for i,v in ipairs(t_paths) do
			coor = coor.."["..table.concat(v, ",").."],"
		end
		coor = string.sub(coor,1,-2).."]"
	end
	local t = {X=x, Y=y, COORDINATES=coor}
	update_content_t(obj, scad_polygon, t)
	return obj
end
function cad.polygon(x,y, t_points, t_paths)
	local t_p = geometry.polygon(t_points)
	return polygon_raw(x,y, t_p, t_paths)
end

--[[--------------------------------------------------------------------------------
	GEOMETRIC 2D OBJECTS
--]]--------------------------------------------------------------------------------

--[[----------------------------------------
	function cad.rectround(x, y, width, height, radius [, center])

	draw rectangle width given width and height and rounded edges according to radius
--]]----------------------------------------
function cad.rectround(x,y, width,height, radius, center)
	if center then
		x = x-width/2
		y = y-height/2
	end
	local obj = cad_obj():add(cad.circle(radius,radius, radius),
		cad.circle(width-radius,radius, radius),
		cad.circle(width-radius,height-radius, radius),
		cad.circle(radius,height-radius, radius))
		:translate(x,y)
		:hull()
	return obj
end

--[[----------------------------------------
	function cad.rectchamfer(x, y, width, height, chamfer [, center])

	draw rectangle width given width and height and chamfered edges
--]]----------------------------------------
function cad.rectchamfer(x,y, width,height, chamfer, center)
	if center then
		x = x-width/2
		y = y-height/2
	end
	local obj = cad_obj()
	-- make 45� angle edges
	local t_points = {{0,chamfer},{chamfer,0},
		{width-chamfer,0}, {width, chamfer},
		{width,height-chamfer}, {width-chamfer, height},
		{chamfer, height}, {0, height-chamfer},
		}

	local obj = cad.polygon(x,y, t_points)
	return obj
end

--[[------------------------------------------
	function cad.longhole(x1, y1, x2, y2, radius)

	draws a long hole
--]]------------------------------------------
local scad_longhole =[[
hull()
{
	translate([$X1,$Y1,0])
	circle($RADIUS);
	translate([$X2,$Y2,0])
	circle($RADIUS);
}
]]
function cad.longhole(x1,y1, x2, y2, radius)
	local obj = cad_obj()
	local t = {X1=x1, Y1=y1, X2=x2, Y2=y2, RADIUS=radius}
	update_content_t(obj, scad_longhole, t)
	return obj
end

--[[------------------------------------------
	function cad.longholerelative(x, y, dx, dy, radius)

	x,y: first circle hole
	dx,dy: coordinates of the second hole relative to x and y

	draws a long hole
--]]------------------------------------------
function cad.longholerelative(x, y, dx, dy, radius)
	return cad.longhole(x,y, x+dx, y+dy, radius)
end


--[[----------------------------------------------------------------------------
	3D OBJECTS
--]]----------------------------------------------------------------------------

--[[------------------------------------------
	function cad.sphere(x, y, z, radius)

	draw a sphere
--]]------------------------------------------
local scad_sphere =[[
translate([$X,$Y,$Z])
sphere(r = $RADIUS);
]]
function cad.sphere(x, y, z, radius)
	local obj = cad_obj()
	local t = {X=x, Y=y, Z=z, RADIUS=radius}
	update_content_t(obj, scad_sphere, t)
	return obj
end

--[[------------------------------------------
	function cad.cube(x, y, z, width, height, depth)

	draw a cube
--]]------------------------------------------
local scad_cube =[[
translate([$X,$Y,$Z])
cube([$WIDTH,$HEIGHT,$DEPTH]);
]]
function cad.cube(x, y, z, width, height, depth)
	local obj = cad_obj()
	local t = {X=x, Y=y, Z=z, WIDTH=width, HEIGHT=height, DEPTH=depth}
	update_content_t(obj, scad_cube, t)
	return obj
end

--[[------------------------------------------
	function cad.cylinder(x, y, z, h, r1, r2)
	function cad.cylinder(x, y, z, h, r1)

	draw a cylinder
--]]------------------------------------------
local scad_cylinder =[[
translate([$X,$Y,$Z])
cylinder($H,$R1,$R2);
]]
function cad.cylinder(x, y, z, h, r1, r2)
	local obj = cad_obj()
	local r2 = r2 or r1
	local t = {X=x, Y=y, Z=z, H=h, R1=r1, R2=r2}
	update_content_t(obj, scad_cylinder, t)
	return obj
end

--[[------------------------------------------
	function cad.polyhedron(x,y,z, t_points, t_faces)

	draw a polygon from given points {{x,y,z},{x,y,z},...,{x,y,z}}
	t_faces: vector of point n-tuples with n >= 3. Each number is the 0-indexed point number from the point vector.
		That is, faces=[ [0,1,4] ] specifies a triangle made from the first, second, and fifth point listed in points.
		When referencing more than 3 points in a single tuple, the points must all be on the same plane.
		defines the faces, the points have to be ordered in counter clock
	note angles are the outer angles measured
--]]------------------------------------------
local scad_polyhedron =[[
translate([$X,$Y,$Z])
polyhedron($COORDINATES, faces=$FACES);
]]
function cad.polyhedron(x,y,z, t_points, t_faces)
	local obj = cad_obj()
	-- create coordinates string
	local coor = "["
	for i,v in ipairs(t_points) do
		coor = coor.."["..v[1]..","..v[2]..","..v[3].."],"
	end
	coor = string.sub(coor,1,-2).."]"
	local faces = "["
	for i,v in ipairs(t_faces) do
		faces = faces.."["..table.concat(v, ",").."],"
	end
	faces = string.sub(faces,1,-2).."]"
	local t = {X=x, Y=y, Z=z, COORDINATES=coor, FACES=faces}
	update_content_t(obj, scad_polyhedron, t)
	return obj
end

--[[------------------------------------------------------------------------------------

	function cad.text(x ,y, z, text [, height [, font [, style [, h_align [, v_align] ] ] ] ])

	height = height in mm or 10 by default
	font = "Arial", "Courier New", "Liberation Sans", etc., default "Arial"
	style = depending on the font, usually "Bold","Italic", etc. default normal
	h_aling = horizontal alignment, Possible values are "left", "center" and "right". Default is "left".
	v_valign = vertical alignment for the text. Possible values are "top", "center", "baseline" and "bottom". Default is "baseline".

--]]------------------------------------------------------------------------------------

local scad_text =[[
translate([$X,$Y,$Z])
text("$TEXT", size=$H, font="$FONT", halign="$HALIGN", valign="$VALIGN");
]]
function cad.text(x,y,z, text, height, font, style, h_align, v_align)
	local height = height or 10
	local font = font or "Arial"
	local style = style and (":"..style) or ""
	local h_align = h_align or "left"
	local v_align = v_align or "baseline"
	local t = {X=x, Y=y, Z=z, TEXT=tostring(text), H=height, FONT=font..style, HALIGN=h_align, VALIGN=v_align}
	local obj = cad_obj()
	update_content_t(obj, scad_text, t)
	return obj
end

--[[------------------------------------------------------------------------------------

	function cad.text3d(x ,y, z, text [, height [, depth [, font [, style [, h_align [, v_align] ] ] ] ] ])

	height = height in mm or 1 by default
	depth = 1 by default
	font = "Arial", "Courier New", "Liberation Sans", etc., default "Arial"
	style = depending on the font, usually "Bold","Italic", etc. default normal
	h_aling = horizontal alignment, Possible values are "left", "center" and "right". Default is "left".
	v_valign = vertical alignment for the text. Possible values are "top", "center", "baseline" and "bottom". Default is "baseline".

--]]------------------------------------------------------------------------------------
function cad.text3d(x,y,z, text, height, depth, font, style, h_align, v_align)
	local obj = cad.text(0,0,0, text, height, font, style, h_align, v_align)
		:linearextrude(depth or 1)
		:translate(x,y,z)
	return obj
end

--[[--------------------------------------------------------------------------------
	GEOMETRIC 3D OBJECTS
--]]--------------------------------------------------------------------------------

--[[----------------------------------------
	function cad.pyramid(x,y,z, length, height)

	draws a pyramid with length as bottom side length and height
--]]----------------------------------------
function cad.pyramid(x,y,z, length, height)
	local t_points = {
		{0,0,0},{length,0,0},{length,length,0},{0,length,0},
		{length/2,length/2,height}
	}
	local t_faces = {
		{0,1,4},{1,2,4},{2,3,4},{3,0,4},{3,2,1,0}
	}
	local obj = cad.polyhedron(x,y,z, t_points, t_faces)
	return obj
end

--[[--------------------------------------------------------------------------------
	CAD AUX OBJECT FUNCTIONS
--]]--------------------------------------------------------------------------------

--[[--------------------------------------------------------------------------------
	GLOBAL OBJ FUNCTIONS via METATABLE
--]]--------------------------------------------------------------------------------
cad_meta.__index = {}

--[[----------------------------------------
	function <cad>:init()

	resets the object
--]]----------------------------------------
cad_meta.__index.init = function(obj)
	obj.scad_content = ""
	obj.segments = default_segments
	obj.intend_num = 0
	-- diffuse color [0,1]
	obj.color = {1,1,1}
	return obj
end
cad_meta.__index.reset = cad_meta.__index.init

--[[----------------------------------------
	function <cad>:copy()

	copies the object
--]]----------------------------------------
cad_meta.__index.copy = function(obj)
	local copy = cad_obj()
	for i,v in pairs(obj) do
		copy[i] = v
	end
	return copy
end
cad_meta.__index.clone = cad_meta.__index.copy

--[[----------------------------------------
	function <cad>:setcirclefragments(num)

	optional $fn and $fs can be set (http://en.wikibooks.org/wiki/OpenSCAD_User_Manual/Other_Language_Features#Special_variables)
--]]----------------------------------------
cad_meta.__index.setcirclefragments = function(obj, fn)
	obj.segments = fn
	return obj
end

--[[----------------------------------------
	function <cad>:setcolor(R, G, B)
	function <cad>:setcolor(color_name)
	function <cad>:setcolor({R, G, B})

	Set an object color, will work with obj and wrl format
	R,G,B are in the range of [0,1]
--]]----------------------------------------
cad_meta.__index.setcolor = function(obj, R, G, B)
	if type(R) == "table" then
		obj.color = R
	elseif type(R) == "string" then
		if color_rgb[R] then
			obj.color = color_rgb[R]
		else
			error("Unable to set color: "..R)
		end
	else
		obj.color = {R, G, B}
	end
	return obj
end

--[[----------------------------------------
	function <cad>:export(file [, color [, verbose] ])
	function <cad>:export(<file>.svg [, {width,height,{color_fill},{color_stroke},stroke_width} [, verbose] ])

	Export 2D: dxf, svg
	Export 3D: stl
	Export 3D color: wrl, x3d, obj

	Color is 8bit RGB
	color[R,G,B] = {[0,1], [0,1], [0,1]}

	Export to dxf, svg, stl or obj with color color = {r,g,b}
--]]----------------------------------------
local t_ext = {
	-- 2D
	[".dxf"] = function(obj, file)
		export_scad(obj, file)
	end,
	[".svg"] = function(obj, file)
		export_scad(obj, file)
		local color = {math.floor(obj.color[1]*255),math.floor(obj.color[2]*255),math.floor(obj.color[3]*255)}
		if color then
			local w = color[1] or "100%%"
			local h = color[2] or "100%%"
			local fill = color[3] or {0,0,255}
			local stroke = color[4] or {0,0,0}
			local stroke_width = color[5] or 0.0
			local svg = io.open(file, "r")
			local content = svg:read("*a")
			svg:close()
			--content = string.gsub(content, "<title>OpenSCAD Model</title>\n", "")
			content = string.gsub(content, "%sheight=\"%d+\"", " height=\""..h.."\"")
			content = string.gsub(content, "%swidth=\"%d+\"", " width=\""..w.."\"")
			content = string.gsub(content, "fill=\"lightgray\"", "fill=\"#"..string.format("%02X%02X%02X", fill[1],fill[2],fill[3]).."\"")
			content = string.gsub(content, "stroke=\"black\"", "stroke=\"#"..string.format("%02X%02X%02X", stroke[1],stroke[2],stroke[3]).."\"")
			content = string.gsub(content, "stroke%-width=\"0%.5\"", "stroke-width=\""..stroke_width.."\"")
			local svg = io.open(file, "w")
			svg:write(content)
			svg:close()
		end
	end,
	-- 3D
	[".scad"] = function(obj, file)
		export_scad(obj, file)
	end,
	[".stl"] = function(obj, file)
		export_scad(obj, file)
	end,
	[".obj"] = function(obj, file)
		-- export as stl
		export_scad(obj, file..".stl")
		local color = color or "white"
		cad.stl.toobj(file, {file..".stl"}, {obj.color})
		os.remove(file..".stl")
	end,
	[".wrl"] = function(obj, file)
		-- export as stl
		export_scad(obj, file..".stl")
		local color = color or "white"
		cad.stl.towrl(file, {file..".stl"}, {obj.color})
		os.remove(file..".stl")
	end,
}
cad_meta.__index.export = function(obj, file, verbose)
	if verbose then
		print("---------------------------------")
		print("cad.export: -> "..file)
		print()
		print(cad.openscad.include)
		print("$fn = "..obj.segments..";")
		print(obj.scad_content)
		print()
		print("scad_file: "..scadfile)
		print("---------------------------------")
	end

	-- check extension
	local ext = getExtension(file)
	local f = t_ext[ext]
	if not f then
		error("<cad>:export, unknown extension "..ext)
	end
	print("Exporting to \""..file.."\"")
	f(obj, file)
	return obj
end

--[[----------------------------------------
	function <cad>:native(code)

	-- for executing native code
--]]----------------------------------------
function cad_meta.__index.native(obj, str)
	update_content(obj, "\n//native\n"..str.."\n")
	return obj
end

--[[----------------------------------------
	function <cad>:import(file)

	import an stl/dfx here
--]]----------------------------------------
function cad_meta.__index.import(obj, file)
	update_content(obj, "import("..string.format("%q",file)..");\n")
	return obj
end

--[[--------------------------------------------------------------------------------
	CAD TRANSFORMING OBJECT FUNCTIONS
--]]--------------------------------------------------------------------------------

--[[----------------------------------------
	function <cad>:union()

	union the cad object
--]]----------------------------------------
function cad_meta.__index.union(obj_1)
	local obj = cad_obj()
	update_content(obj, "union()\n{\n")
	intend_content(obj, 1)
	update_content(obj, obj_1.scad_content)
	intend_content(obj, -1)
	update_content(obj, "}\n")
	obj_1.scad_content = obj.scad_content
	return obj_1
end

--[[----------------------------------------
	function <cad>:sub(obj_1 [, obj_2 [, ..., obj_n] ])

	substract multiple objects from the current object
--]]----------------------------------------
function cad_meta.__index.sub(obj_1, ...)
	local obj = cad_obj()
	local obj_s = {...}
	update_content(obj, "difference()\n{\n")
	intend_content(obj, 1)
	update_content(obj, cad_meta.__index.union(obj_1).scad_content)
	for i,v in ipairs(obj_s) do
		update_content(obj, v.scad_content)
	end
	intend_content(obj, -1)
	update_content(obj, "}\n")
	obj_1.scad_content = obj.scad_content
	return obj_1
end

--[[----------------------------------------
	function <cad>:add(obj_1 [, obj_2 [, ..., obj_n] ])

	add multiple objects to the current object
--]]----------------------------------------
function cad_meta.__index.add(obj_1, ...)
	local obj = cad_obj()
	update_content(obj, obj_1.scad_content)
	local obj_s = {...}
		for i,v in ipairs(obj_s) do
		update_content(obj, v.scad_content)
	end
	obj_1.scad_content = obj.scad_content
	return obj_1
end

--[[----------------------------------------
	function <cad>:intersect(obj_1 [, obj_2 [, ..., obj_n] ])

	intersect multiple objects with the current object
--]]----------------------------------------
function cad_meta.__index.intersect(obj_1, ...)
	local obj = cad_obj()
	local obj_s = {...}
	update_content(obj, "intersection()\n{\n")
	intend_content(obj, 1)
	update_content(obj, obj_1.scad_content)
	for i,v in ipairs(obj_s) do
		update_content(obj, v.scad_content)
	end
	intend_content(obj, -1)
	update_content(obj, "}\n")
	obj_1.scad_content = obj.scad_content
	return obj_1
end

--[[----------------------------------------
	fuction <cad>:scale(x,y,z)

	scale an object in percent/100, 1=same size, 2=double, 0.5=half
--]]----------------------------------------
function cad_meta.__index.scale(obj_1, x,y,z)
	local obj = cad_obj()
	update_content(obj, "scale(["..x..","..y..","..z.."])\n{\n")
	intend_content(obj, 1)
	update_content(obj, obj_1.scad_content)
	intend_content(obj, -1)
	update_content(obj, "}\n")
	obj_1.scad_content = obj.scad_content
	return obj_1
end

--[[----------------------------------------
	fuction <cad>:resize(x,y,z)

	resize an object in x,y,z if any of the axes is zero will automatically scale
--]]----------------------------------------
function cad_meta.__index.resize(obj_1, x,y,z, auto_x,auto_y,auto_z)
	local auto_x = (not auto_x and false) or true
	local auto_y = (not auto_y and false) or true
	local auto_z = (not auto_z and false) or true
	local obj = cad_obj()
	update_content(obj, "resize(["..x..","..y..","..z.."], auto=["..tostring(auto_x)..","..tostring(auto_y)..","..tostring(auto_z).."])\n{\n")
	intend_content(obj, 1)
	update_content(obj, obj_1.scad_content)
	intend_content(obj, -1)
	update_content(obj, "}\n")
	obj_1.scad_content = obj.scad_content
	return obj_1
end

--[[------------------------------------------
	function <cad>:translate(x, y [, z])

	z is by default 0.
	move the objects relative by x,y and z
--]]------------------------------------------
function cad_meta.__index.translate(obj_1, x, y, z)
	local z = z or 0
	local obj = cad_obj()
	update_content(obj, "translate(["..x..","..y..","..z.."])\n{\n")
	intend_content(obj, 1)
	update_content(obj, obj_1.scad_content)
	intend_content(obj, -1)
	update_content(obj, "}\n")
	obj_1.scad_content = obj.scad_content
	return obj_1
end

--[[------------------------------------------
	function <cad>:mirror(x, y, z)

	mirror the objects according to setup vector plane defined via x,y,z
	z is 0 by default
	values 1 or 0
--]]------------------------------------------
function cad_meta.__index.mirror(obj_1, x, y, z)
	local z = z or 0
	local obj = cad_obj()
	update_content(obj, "mirror(["..x..","..y..","..z.."])\n{\n")
	intend_content(obj, 1)
	update_content(obj, obj_1.scad_content)
	intend_content(obj, -1)
	update_content(obj, "}\n")
	obj_1.scad_content = obj.scad_content
	return obj_1
end

--[[------------------------------------------
	function <cad>:rotate(x, y, z, x_angle, y_angle, z_angle)

	x,y and z are the rotation point
--]]------------------------------------------
function cad_meta.__index.rotate(obj_1, x, y, z, x_angle, y_angle, z_angle)
	local obj = cad_obj()
	update_content(obj, "translate(["..(x)..","..(y)..","..(z).."])\nrotate(["..x_angle..","..y_angle..","..z_angle.."])\ntranslate(["..(-x)..","..(-y)..","..(-z).."])\n{\n")
	intend_content(obj, 1)
	update_content(obj, obj_1.scad_content)
	intend_content(obj, -1)
	update_content(obj, "}\n")
	obj_1.scad_content = obj.scad_content
	return obj_1
end

--[[------------------------------------------
	function <cad>:rotate2d(x, y, angle)
	function rotate2d_end()

	x and y are the rotation point
--]]------------------------------------------
function cad_meta.__index.rotate2d(obj_1, x,y, angle)
	local obj = cad_obj()
	update_content(obj, "translate(["..(x)..","..(y)..",0])\nrotate([0,0,"..angle.."])\ntranslate(["..(-x)..","..(-y)..",0])\n{\n")
	intend_content(obj, 1)
	update_content(obj, obj_1.scad_content)
	intend_content(obj, -1)
	update_content(obj, "}\n")
	obj_1.scad_content = obj.scad_content
	return obj_1
end

--[[------------------------------------------
	function <cad>:rotateextrude()

	the object has to moved away from the center for rotate_extrude to succeed
--]]------------------------------------------
function cad_meta.__index.rotateextrude(obj_1)
	local obj = cad_obj()
	update_content(obj, "rotate_extrude()\n{\n")
	intend_content(obj, 1)
	update_content(obj, obj_1.scad_content)
	intend_content(obj, -1)
	update_content(obj, "}\n")
	obj_1.scad_content = obj.scad_content
	return obj_1
end

--[[------------------------------------------
	function <cad>:linearextrude(depth [, twist [, slices [, scale] ] ])

	note linear extrude moves the object back to the ground
--]]------------------------------------------
function cad_meta.__index.linearextrude(obj_1, depth, twist, slices, scale)
	local twist = twist or 0 -- in degrees
	local slices = slices or 1 -- in how many slices it should be put
	local scale = scale or 1.0 -- if the object should be scaled towards extrusion
	--local convexity = convexity or 10
	local obj = cad_obj()
	update_content(obj, "linear_extrude(height = "..depth..", twist = "..twist..", slices = "..slices..", scale = "..scale..", center = false)\n{\n")
	--update_content(obj, "linear_extrude(height = "..depth..", twist = "..twist..", slices = "..slices..", scale = "..scale..", convexity = "..convexity..", center = false)\n{\n")
	intend_content(obj, 1)
	update_content(obj, obj_1.scad_content)
	intend_content(obj, -1)
	update_content(obj, "}\n")
	obj_1.scad_content = obj.scad_content
	return obj_1
end

--[[------------------------------------------
	function <cad>:hull()

	hull the cad object
--]]------------------------------------------
function cad_meta.__index.hull(obj_1)
	local obj = cad_obj()
	update_content(obj, "hull()\n{\n")
	intend_content(obj, 1)
	update_content(obj, obj_1.scad_content)
	intend_content(obj, -1)
	update_content(obj, "}\n")
	obj_1.scad_content = obj.scad_content
	return obj_1
end

--[[------------------------------------------
	function <cad>:offset(d, chamfer)

	Offset the object with distance d
--]]------------------------------------------
function cad_meta.__index.offset(obj_1, d, chamfer)
	local obj = cad_obj()
	local chamfer = (chamfer and true) or false
	update_content(obj, "offset(d="..r..",chamfer="..chamfer..")\n{\n")
	intend_content(obj, 1)
	update_content(obj, obj_1.scad_content)
	intend_content(obj, -1)
	update_content(obj, "}\n")
	obj_1.scad_content = obj.scad_content
	return obj_1
end

--[[------------------------------------------
	function <cad>:offsetradius(r, chamfer)

	Offset the object with radius r
--]]------------------------------------------
function cad_meta.__index.offsetradius(obj_1, r, chamfer)
	local obj = cad_obj()
	local chamfer = (chamfer and true) or false
	update_content(obj, "offset(r="..r..",chamfer="..chamfer..")\n{\n")
	intend_content(obj, 1)
	update_content(obj, obj_1.scad_content)
	intend_content(obj, -1)
	update_content(obj, "}\n")
	obj_1.scad_content = obj.scad_content
	return obj_1
end


cad_meta.__add = function (a,b)
	return a:add(b)
end

cad_meta.__sub = function (a,b)
	return a:sub(b)
end

--[[--------------------------------------------------------------------------------
	CAD UTILITY Functions
--]]--------------------------------------------------------------------------------


--[[----------------------------------------
	function cad.export(filename, obj_1 [, obj_2, obj_n])
	exports all objects into one file

	Export 2D: dxf, svg
	Export 3D: scad, stl
	Export 3D color: wrl, x3d, obj

	Export to dxf, svg, stl or obj with color color = {r,g,b}
--]]----------------------------------------
local f_ext = {
	-- 2D
	[".dxf"] = function(file, objs)
		local obj = objs[1]
		for i=2,#objs do
			obj:add(objs[i])
		end
		obj:export(file)
	end,
	[".svg"] = function(file, objs)
		local obj = objs[1]
		for i=2,#objs do
			obj:add(objs[i])
		end
		obj:export(file)
	end,
	[".scad"] = function(file, objs)
		local obj = objs[1]
		for i=2,#objs do
			obj:add(objs[i])
		end
		obj:export(file)
	end,
	[".stl"] = function(file, objs)
		local obj = objs[1]
		for i=2,#objs do
			obj:add(objs[i])
		end
		obj:export(file)
	end,
	[".obj"] = function(file, objs)
		local t_files = {}
		local t_colors = {}
		for i,v in ipairs(objs) do
			local fname = cad.settings.tmpfile.."_"..i..".stl"
			v:export(fname)
			table.insert(t_files, fname)
			table.insert(t_colors, v.color)
		end
		cad.stl.toobj(file, t_files, t_colors)
		for i,v in ipairs(t_files) do
			os.remove(v)
		end
	end,
	[".wrl"] = function(file, objs)
		local t_files = {}
		local t_colors = {}
		for i,v in ipairs(objs) do
			local fname = cad.settings.tmpfile.."_"..i..".stl"
			v:export(fname)
			table.insert(t_files, fname)
			table.insert(t_colors, v.color)
		end
		cad.stl.towrl(file, t_files, t_colors)
		for i,v in ipairs(t_files) do
			os.remove(v)
		end
	end,
}

function cad.export(filename, ...)
	local objs = {...}
	-- check extension
	local ext = getExtension(filename)
	print("ext", ext)
	local f = f_ext[ext]
	if not f then
		error("cad.export, unknown extension "..ext)
	end
	f(filename, objs)
end

--[[--------------------------------------------------------------------------------
	STL FUNCTIONS
--]]--------------------------------------------------------------------------------

cad.stl = {}

function cad.stl.write(filename, s_stl)
	local f,err = io.open(filename, "w")
	if err then error(err) end
	f:write(s_stl)
	f:close()
end

--/////////////////////////////////////////////////////////////////
-- import ASCII STL file
-- stl files consist of a normal and three points aranged in counter clock
-- returns all the faces {{normal,v1,v2,v3},...}
-- where normal and v... are a table holding three floating point numbers e.g. {num1,num2,num3}
-- does not perform any checking on the file itself
local function cad_import_stl(filename)
	local file_stl,err = io.open(filename, "r")
	if err then error(err) end
	local t_faces = {}
	local cur_face = {}
	local count = 0
	for line in file_stl:lines() do
		if string.find(line, "^%s*facet normal") then
			local x,y,z = string.match(line, "(%S+)%s(%S+)%s(%S+)$")
			table.insert(cur_face, {tonumber(x),tonumber(y),tonumber(z)})
		end
		if string.find(line, "^%s*vertex") then
			local x,y,z = string.match(line, "(%S+)%s(%S+)%s(%S+)$")
			table.insert(cur_face, {tonumber(x),tonumber(y),tonumber(z)})
			count = count + 1
			if count == 3 then
				table.insert(t_faces,cur_face)
				cur_face = {}
				count = 0
			end
		end
	end
	file_stl:close()
	return t_faces
end

--[[
	Convert ASCII STL file to OBJ (wavefront file) with optional coloring
	Info:

	Imports an ASCII stl file into a t_faces structure
	Exports a t_faces structure into an OBJ file with optional coloring
--]]
--/////////////////////////////////////////////////////////////////
-- export t_faces structure to OBJ (wavefront) file
-- filename: full filepath to obj file
-- t_faces a t_faces structure
-- color is optional and has to be a table holding three r,g,b, numbers e.g. {[0,1],[0,1],[0,1]} or a string e.g. "white"
cad.stl.toobj = function(obj_filename, t_stl_filenames, t_colors)

	-- strip only filename
	local filename = string.match(obj_filename,"([^\\/]+)%.%S%S%S$")

	-- if color then write mtl
	local mtl_filename = filename..".mtl"
	local file,err = io.open(mtl_filename, "w")
	if err then error(err) end
	local t_mtl_color = {}
	for i,v in ipairs(t_colors) do
		--print(v)
		local col = {}
		if color_rgb[v] then
			color = color_rgb[v]
		else
			color = {v[1],v[2],v[3]}
		end
		local mtl_color = "color_["..string.format("%.06f-%.06f-%.06f",color[1], color[2], color[3]).."]_"..i
		file:write(
			"# Color: "..mtl_color.."\n"..
			"newmtl "..mtl_color.."\n"..
			"Ka "..string.format("%.06f %.06f %.06f",color[1], color[2], color[3]).."\n"..
			"Kd "..string.format("%.06f %.06f %.06f",color[1], color[2], color[3]).."\n"..
			"Ks 0.000000 0.000000 0.000000\n"..
			"Ns 2.000000\n"..
			"d 1.000000\n"..
			"Tr 1.000000\n\n")
		table.insert(t_mtl_color, mtl_color)
	end
	file:close()

	-- get all vertices
	local t_faces = {}
	for i,v in ipairs(t_stl_filenames) do
		table.insert(t_faces, cad_import_stl(v))
	end

	-- write obj

	local file,err = io.open(obj_filename, "w")
	if err then error(err) end

	file:write("#########################################\n")
	file:write("# Wavefront file\n")
	file:write("# Created via: Lua_CAD (https://github.com/mvlutz/Lua_CAD)\n")
	file:write("# Date: "..os.date().."\n")
	file:write("#########################################\n\n")

	if color then
		file:write("#########################################\n")
		file:write("# MTL lib:\n\n")
		file:write("mtllib "..mtl_filename.."\n\n")
	end

	-- List of Vertices
	file:write("#########################################\n")
	file:write("# List of vertices:\n\n")

	for _,t_v in ipairs(t_faces) do
		for i,v in ipairs(t_v) do
			for i2=2,4 do
				file:write("v "..string.format("%f",v[i2][1]).." "..string.format("%f",v[i2][2]).." "..string.format("%f",v[i2][3]).."\n")
			end
		end
	end
	file:write("\n")

	-- Normals in (x,y,z) form; normals might not be unit.
	-- Normals are not needed since stl saves the triangles in counter clock anyways
	--file:write("\n# List of Normals\n\n")
	--for i,v in ipairs(t_faces) do
	--	file:write("vn "..string.format("%f",v[1][1]).." "..string.format("%f",v[1][2]).." "..string.format("%f",v[1][3]).."\n")
	--end

	-- Face Definitions (see below)
	local count = 0
	for i,v in ipairs(t_mtl_color) do

		file:write("\n#########################################\n")
		file:write("# Face definitions:\n\n")

		if color then
			file:write("usemtl "..v.."\n\n")
		end

		for i,v in ipairs(t_faces[i]) do
			local i2 = (count)*3
			--file:write("f "..(i2+1).."//"..i.." "..(i2+2).."//"..i.." "..(i2+3).."//"..i.."\n")
			file:write("f "..(i2+1).." "..(i2+2).." "..(i2+3).."\n")
			count = count + 1
		end
	end
	file:close()
end

--/////////////////////////////////////////////////////////////////
-- export stl to wrl 2.0
-- filename: full filepath to obj file
-- color is optional and has to be a table holding three r,g,b, numbers e.g. {[0,1],[0,1],[0,1]} or a string e.g. "white"
cad.stl.towrl = function(wrl_filename, t_stl_filenames, t_colors)

	-- strip only filename
	local filename = string.match(wrl_filename,"([^\\/]+)%.%S%S%S$")

	-- write wrl
	local file,err = io.open(wrl_filename, "w")
	if err then error(err) end

	-- set utf8 syntax
	file:write("#VRML V2.0 utf8\n")
	file:write("\n")
	file:write("# Created via: Lua_CAD (https://github.com/mvlutz/Lua_CAD)\n")
	file:write("# Date: "..os.date().."\n")

	-- for each stl file write a shape
	-- get all vertices
	for i,v in ipairs(t_stl_filenames) do
		local t_faces = cad_import_stl(v)
		file:write("\nShape {\n")
		local col = t_colors[i]
		if col then
			if color_rgb[col] then
				col = color_rgb[v]
			else
				col = {col[1],col[2],col[3]}
			end
			file:write("\tappearance Appearance {\n")
			file:write("\t\tmaterial Material {\n")
			file:write("\t\t\tdiffuseColor "..string.format("%.06f %.06f %.06f",col[1], col[2], col[3]).."\n")
			file:write("\t\t}\n")
			file:write("\t}\n")
		end

		-- List of Vertices
		file:write("\tgeometry IndexedFaceSet {\n\n")
		file:write("#\t\tList of Coordinates:\n")
		file:write("\t\tcoord Coordinate {\n")
		file:write("\t\t\tpoint [\n")
		for i,v in ipairs(t_faces) do
			for i2=2,4 do
				file:write("\t\t\t\t"..string.format("%f",v[i2][1]).." "..string.format("%f",v[i2][2]).." "..string.format("%f",v[i2][3]).."\n")
			end
		end
		file:write("\t\t\t]\n")
		file:write("\t\t}\n\n")
		file:write("#\t\tList of Faces:\n")
		file:write("\t\tcoordIndex [\n")
		local count = 0
		for _,t_v in ipairs(t_faces) do
			file:write("\t\t\t"..(count).." "..(count+1).." "..(count+2).." -1\n")
			count = count + 3
		end
		file:write("\t\t]\n")
		file:write("\t}\n")
		file:write("}\n")
	end
	file:close()
end


function cad.stl.cube(x,y,z, width,height,depth)

	local w,h,d = width,height,depth

	local vertex = {
		{	{-1, 0, 0},
			{0, 0, d},
			{0, h, d},
			{0, 0, 0}
		},
		{	{-1, 0, 0},
			{0, 0, 0},
			{0, h, d},
			{0, h, 0}
		},
		{	{0, 0, 1},
			{0, 0, d},
			{w, 0, d},
			{w, h, d},
		},
		{	{0, 0, 1},
			{0,h,d},
			{0,0,d},
			{w,h,d},
		},
		{	{0,-1,0},
			{0,0,0},
			{w,0,0},
			{w,0,d},
		},
		{	{0,-1,0},
			{0,0,d},
			{0,0,0},
			{w,0,d},
		},
		{	{0,0,-1},
			{0,h,0},
			{w,h,0},
			{0,0,0},
		},
		{	{0,0,-1},
			{0,0,0},
			{w,h,0},
			{w,0,0},
		},
		{	{0,1,0},
			{0,h,d},
			{w,h,d},
			{0,h,0},
		},
		{	{0,1,0},
			{0,h,0},
			{w,h,d},
			{w,h,0},
		},
		{	{1,0,0},
			{w,0,0},
			{w,h,0},
			{w,h,d},
		},
		{	{1,0,0},
			{w,0,d},
			{w,0,0},
			{w,h,d},
		}
	}

	-- create stl string
	local indent = "  "
	local stl = "solid cube\n"
	for i,v in ipairs(vertex) do
		stl = stl..indent.."facet normal "..v[1][1].." "..v[1][2].." "..v[1][3].."\n"
		stl = stl..indent..indent.."outer loop\n"
		stl = stl..indent..indent..indent.."vertex "..(x+v[2][1]).." "..(y+v[2][2]).." "..(z+v[2][3]).."\n"
		stl = stl..indent..indent..indent.."vertex "..(x+v[3][1]).." "..(y+v[3][2]).." "..(z+v[3][3]).."\n"
		stl = stl..indent..indent..indent.."vertex "..(x+v[4][1]).." "..(y+v[4][2]).." "..(z+v[4][3]).."\n"
		stl = stl..indent..indent.."endloop\n"
	end
	stl = stl.."ensolid cube\n"

	return stl
end
