-- Front bodywork: the centre panel with the camera bay cut into it, the
-- four side panels that fair it into the trays, and the plate the two
-- RealSense cameras bolt to.
--
-- Ported from v3/scad/racecar_front_cover_center.scad,
-- racecar_front_cover_front_left_side.scad, _front_right_side.scad,
-- _back_left_side.scad and _back_right_side.scad.
--
-- Everything is in front-foundation coordinates. The panels are sheets
-- stretched between two edges: the rear cover's top panel at the back, and
-- the front tray's kerbs at the front. Almost every vertex below is one of
-- those two edges, or a point interpolated along the slope between them.
--
-- The original insets a race number into the centre panel and lettering
-- into the two rear side panels; those are dropped.

local u = require("parts.utils")
local back_foundation = require("parts.back_foundation")
local back_cover = require("parts.back_cover")
local ff = require("parts.front_foundation")

local M = {}

--------------------------------------------------------------------------
-- Back edge: the rear cover's top panel, seen from up front
--------------------------------------------------------------------------

local rbct_x = -ff.x + back_foundation.x + back_cover.length / 2
local rbct_left_y = back_cover.width / 2 + 2.0
local rbct_right_y = -rbct_left_y
local rbct_top_z = -ff.z
  + back_foundation.z
  + back_cover.z
  + back_cover.height / 2
local rbct_bottom_z = -ff.z
  + back_foundation.z
  + back_cover.z
  - back_cover.height / 2

--------------------------------------------------------------------------
-- Front edge: the front tray's kerbs
--------------------------------------------------------------------------

-- Where the centre panel lands: the kerb's front end.
local cover_front_y = ff.head_front_y
local cover_back_x = ff.wall_front_outer_x
local cover_back_y = ff.wall_front_outer_y

-- Where the side panels land: the kerb's rear end. Distinct from the pair
-- above -- the kerb runs at an angle, so its two ends differ in both x
-- and y, and mixing them up collapses the side panels.
local kerb_back_x = ff.wall_back_outer_x
local kerb_back_y = ff.wall_back_outer_y

--------------------------------------------------------------------------
-- Slopes from back edge to front edge
--------------------------------------------------------------------------

local top_xz_slope = (ff.wall_top_z - rbct_top_z) / (ff.head_front_x - rbct_x)
local bottom_xz_slope = (ff.wall_top_z - rbct_bottom_z) / (cover_back_x - rbct_x)
local left_xy_slope = (cover_front_y - rbct_left_y) / (ff.head_front_x - rbct_x)
-- Reproduced verbatim from the original, including the slip in the
-- denominator: it subtracts the back edge's *y* from the front edge's x.
-- Correcting it moves the right-hand panels off the left-hand ones, so the
-- published model depends on the value as written.
local right_xy_slope = (-cover_front_y - rbct_right_y)
  / (ff.head_front_x - rbct_right_y)

-- A point on the panel surface, `dx` forward of the back edge.
local function along(dx, y0, z0, xy_slope, xz_slope)
  return { rbct_x + dx, y0 + dx * xy_slope, z0 + dx * xz_slope }
end

--------------------------------------------------------------------------
-- Camera bay
--------------------------------------------------------------------------

local cutout_front_dx = 45.0
local cutout_back_dx = cutout_front_dx + 25.0 * (1.0 / top_xz_slope)
local mount_dx = -u.wall_thickness + cutout_back_dx

local cut_front_left_top =
  along(cutout_front_dx, rbct_left_y, rbct_top_z, left_xy_slope, top_xz_slope)
local cut_front_left_bottom = along(
  cutout_front_dx,
  rbct_left_y,
  rbct_bottom_z,
  left_xy_slope,
  bottom_xz_slope
)
local cut_front_right_top = along(
  cutout_front_dx,
  rbct_right_y,
  rbct_top_z,
  right_xy_slope,
  top_xz_slope
)
local cut_front_right_bottom = along(
  cutout_front_dx,
  rbct_right_y,
  rbct_bottom_z,
  right_xy_slope,
  bottom_xz_slope
)

local cut_back_left_top =
  along(cutout_back_dx, rbct_left_y, rbct_top_z, left_xy_slope, top_xz_slope)
local cut_back_left_bottom = along(
  cutout_back_dx,
  rbct_left_y,
  rbct_bottom_z,
  left_xy_slope,
  bottom_xz_slope
)
cut_back_left_bottom[3] = cut_back_left_bottom[3] - 0.01
local cut_back_right_top = along(
  cutout_back_dx,
  rbct_right_y,
  rbct_top_z,
  right_xy_slope,
  top_xz_slope
)
local cut_back_right_bottom = along(
  cutout_back_dx,
  rbct_right_y,
  rbct_bottom_z,
  right_xy_slope,
  bottom_xz_slope
)
cut_back_right_bottom[3] = cut_back_right_bottom[3] - 0.01

-- The lip the cameras bolt through, just inboard of the bay's back edge.
local mount_left_top =
  along(mount_dx, rbct_left_y, rbct_top_z, left_xy_slope, top_xz_slope)
local mount_left_bottom = {
  mount_left_top[1],
  mount_left_top[2],
  cut_front_left_bottom[3],
}
local mount_right_top =
  along(mount_dx, rbct_right_y, rbct_top_z, right_xy_slope, top_xz_slope)
local mount_right_bottom = {
  mount_right_top[1],
  mount_right_top[2],
  cut_front_right_bottom[3],
}

local mount_mid_y = (mount_left_top[2] + mount_right_top[2]) / 2

local camera_screw_height = 2 * u.wall_thickness
local camera_screw_x = (mount_right_top[1] + cut_back_right_top[1]) / 2
local camera_screw_left_y = mount_mid_y + 45 / 2
local camera_screw_right_y = mount_mid_y - 45 / 2
local camera_screw_z = cut_front_right_top[3] + 14.5

local t265_screw_left_y = mount_mid_y + 50.0 / 2
local t265_screw_right_y = t265_screw_left_y - 50.0
local t265_screw_z = camera_screw_z

local d455_screw_left_y = mount_mid_y + 95.0 / 2
local d455_screw_right_y = mount_mid_y - 95.0 / 2
local d455_screw_z = cut_front_right_top[3] + 29.0 / 2 + 2

local d455_head_height = (camera_screw_x + camera_screw_height / 2)
  - rbct_x
local d455_head_x = mount_left_top[1] - d455_head_height / 2 + 0.7

--------------------------------------------------------------------------
-- Camera plate
--------------------------------------------------------------------------

local plate_length = 8.0
local plate_width = -2 * t265_screw_right_y
  + 2 * u.m3_screw_head_radius
  + 4 * u.wall_thickness
  + 35
local plate_height = rbct_top_z
  - (t265_screw_z - u.m3_screw_shaft_radius - u.wall_thickness)
  + 24.5 / 2
  + u.m3_screw_shaft_radius
  + u.wall_thickness
local plate_x = cut_back_left_top[1] + plate_length / 2
local plate_z = t265_screw_z
  - u.m3_screw_shaft_radius
  - u.wall_thickness
  + plate_height / 2

-- Where the two cameras end up, which the assembly needs.
M.plate_front_x = plate_x + plate_length / 2
M.camera_mid_y = (camera_screw_left_y + camera_screw_right_y) / 2
M.camera_screw_z = camera_screw_z
M.t265_screw_z = plate_z
  + plate_height / 2
  - u.wall_thickness
  - u.m3_screw_shaft_radius
  + 4

--------------------------------------------------------------------------
-- Shared cutting tools
--------------------------------------------------------------------------

-- The camera bay itself: a tapered box following the panel's slope.
local function bay_cutout()
  return polyhedron {
    points = {
      cut_front_left_top,
      cut_front_left_bottom,
      cut_front_right_top,
      cut_front_right_bottom,
      cut_back_left_top,
      cut_back_left_bottom,
      cut_back_right_top,
      cut_back_right_bottom,
    },
    faces = {
      { 2, 3, 7, 6 },
      { 1, 0, 4, 5 },
      { 2, 0, 1, 3 },
      { 5, 4, 6, 7 },
      { 4, 0, 2, 6 },
      { 5, 7, 3, 1 },
    },
  }
end

-- Cable exit for the two cameras' USB leads.
local function usb_cutout()
  local length = 10 + plate_length

  return cube { { length, 100.0, 30.0 }, center = true }:translate(
    cut_back_right_top[1] + length / 2,
    cut_front_right_top[2] - 100.0 / 2.1,
    cut_front_right_top[3] + 30.0 / 2
  )
end

-- The bay was originally sized for a D435; this opens it out for the
-- taller D455. Every panel around the bay gets the same block removed.
local function d455_removal()
  local length = cut_front_right_top[1] - cut_back_right_top[1]
  local height = cut_back_right_top[3] - cut_front_right_bottom[3]

  return cube { { length, ff.width, height }, center = true }:translate(
    0.5 * (cut_back_right_top[1] + cut_front_right_top[1]),
    0.5 * (cut_back_left_bottom[2] + cut_back_right_bottom[2]),
    cut_front_right_top[3] + height / 2
  )
end

local function d455_head_bore()
  return u.cyl(d455_head_height, 7.5 / 2):rotate(0, 90, 0)
end

--------------------------------------------------------------------------
-- Centre panel
--------------------------------------------------------------------------

function M.center()
  -- The main shell: back edge at the rear cover, front edge at the kerbs.
  local shell = polyhedron {
    points = {
      { rbct_x, rbct_left_y, rbct_top_z }, -- 0
      { rbct_x, rbct_left_y, rbct_bottom_z }, -- 1
      { rbct_x, rbct_right_y, rbct_top_z }, -- 2
      { rbct_x, rbct_right_y, rbct_bottom_z }, -- 3
      { ff.head_front_x, cover_front_y, ff.wall_top_z }, -- 4
      { ff.head_front_x, -cover_front_y, ff.wall_top_z }, -- 5
      { cover_back_x, cover_back_y, ff.wall_top_z }, -- 6
      { cover_back_x, -cover_back_y, ff.wall_top_z }, -- 7
    },
    faces = {
      { 0, 2, 3, 1 },
      { 0, 4, 5, 2 },
      { 1, 3, 7, 6 },
      { 0, 1, 6, 4 },
      -- Warped right flank {3,2,5,7}, split 2-7 to match the original.
      -- Its left-hand mirror above happens to fall on the fan diagonal.
      { 3, 2, 7 },
      { 2, 5, 7 },
      { 4, 6, 7, 5 },
    },
  }

  -- Two passes at the same cut: the original widens one slightly in Y to
  -- clear a sliver its exact-fit twin leaves behind.
  shell = shell
    - bay_cutout():translate(0, 0, -0.03):scale(0.9999, 1.05, 1.001)
    - bay_cutout()

  -- The skirt below the front kerb, closing the gap down to the tray.
  local skirt = polyhedron {
    points = {
      { ff.head_front_x, cover_front_y, ff.wall_top_z }, -- 0
      { ff.head_front_x, -cover_front_y, ff.wall_top_z }, -- 1
      { cover_back_x, cover_back_y, ff.wall_top_z }, -- 2
      { cover_back_x, -cover_back_y, ff.wall_top_z }, -- 3
      { ff.head_front_x, cover_front_y, ff.wall_bottom_z }, -- 4
      { ff.head_front_x, -cover_front_y, ff.wall_bottom_z }, -- 5
      { cover_back_x, cover_back_y, ff.wall_bottom_z }, -- 6
      { cover_back_x, -cover_back_y, ff.wall_bottom_z }, -- 7
    },
    faces = {
      { 0, 1, 3, 2 },
      { 2, 3, 7, 6 },
      { 0, 4, 5, 1 },
      { 0, 2, 6, 4 },
      { 1, 5, 7, 3 },
      { 4, 6, 7, 5 },
    },
  }

  -- The flange the cameras bolt to, standing across the back of the bay.
  local mount = polyhedron {
    points = {
      mount_left_top,
      mount_left_bottom,
      mount_right_top,
      mount_right_bottom,
      cut_back_left_top,
      cut_back_right_top,
      { cut_back_left_top[1], cut_back_left_top[2], mount_left_bottom[3] },
      { cut_back_right_top[1], cut_back_right_top[2], mount_right_bottom[3] },
    },
    faces = {
      { 0, 2, 3, 1 },
      { 5, 4, 6, 7 },
      { 2, 0, 4, 5 },
      { 1, 3, 7, 6 },
      { 4, 0, 1, 6 },
      { 3, 2, 5, 7 },
    },
  }

  -- The floor of the bay, between its front lip and the flange.
  local floor = polyhedron {
    points = {
      cut_front_left_top,
      cut_front_left_bottom,
      cut_front_right_top,
      cut_front_right_bottom,
      { cut_back_left_top[1], cut_back_left_top[2], mount_left_bottom[3] },
      { cut_back_right_top[1], cut_back_right_top[2], mount_right_bottom[3] },
      { cut_back_left_top[1], cut_back_left_top[2], cut_front_left_top[3] },
      { cut_back_right_top[1], cut_back_right_top[2], cut_front_right_top[3] },
    },
    faces = {
      { 0, 1, 3, 2 },
      { 5, 4, 6, 7 },
      { 3, 1, 4, 5 },
      { 0, 2, 7, 6 },
      { 1, 0, 6, 4 },
      { 2, 3, 5, 7 },
    },
  }

  local wall_screw_height = ff.head_front_x - cover_back_x
  local wall_screw_x = (ff.head_front_x + cover_back_x) / 2
  local wall_screw_z = (ff.wall_bottom_z + ff.wall_top_z) / 2

  local m3 = u.cyl(camera_screw_height, u.m3_screw_shaft_radius)
    :rotate(0, 90, 0)
  local m4 = u.cyl(camera_screw_height, u.m4_screw_shaft_radius)
    :rotate(0, 90, 0)
  local head_bore = d455_head_bore()

  return (shell + skirt + mount + floor)
    - u.m2_5_flathead_screw(wall_screw_height)
      :rotate(0, 90, 0)
      :scale(1.001, 1, 1)
      :translate(wall_screw_x, 0, wall_screw_z)
    - m3:translate(camera_screw_x, camera_screw_left_y, camera_screw_z)
    - m3:translate(camera_screw_x, camera_screw_right_y, camera_screw_z)
    - m3:translate(camera_screw_x, t265_screw_left_y, t265_screw_z)
    - m3:translate(camera_screw_x, t265_screw_right_y, t265_screw_z)
    - m4:translate(camera_screw_x, d455_screw_right_y, d455_screw_z)
    - m4:translate(camera_screw_x, d455_screw_left_y, d455_screw_z)
    - head_bore:translate(d455_head_x, d455_screw_right_y, d455_screw_z)
    - head_bore:translate(d455_head_x, d455_screw_left_y, d455_screw_z)
    - usb_cutout():translate(0, 10, -15.0)
end

--------------------------------------------------------------------------
-- Camera plate
--------------------------------------------------------------------------

function M.camera_plate()
  -- Both the T265's own holes and the shared camera holes are drilled at
  -- two heights, so the plate suits either mounting.
  local m3 = u.cyl(plate_length, u.m3_screw_shaft_radius):rotate(0, 90, 0)
  local m4 = u.cyl(camera_screw_height + 20, u.m4_screw_shaft_radius)
    :rotate(0, 90, 0)

  local plate = cube {
    { plate_length, plate_width, plate_height + 5 },
    center = true,
  }:translate(plate_x, 0, plate_z)

  for _, z in ipairs { t265_screw_z, M.t265_screw_z } do
    plate = plate
      - m3:translate(plate_x, t265_screw_left_y, z)
      - m3:translate(plate_x, t265_screw_right_y, z)
      - m3:translate(plate_x, camera_screw_left_y, z)
      - m3:translate(plate_x, camera_screw_right_y, z)
  end

  return plate
    - m4:translate(camera_screw_x, d455_screw_right_y, d455_screw_z)
    - m4:translate(camera_screw_x, d455_screw_left_y, d455_screw_z)
end

--------------------------------------------------------------------------
-- Forward side panels
--
-- Each fairs the centre panel's outer edge down onto the tray's kerb.
--------------------------------------------------------------------------

function M.front_left_side()
  local cover_left = { ff.head_back_x, ff.head_back_y, ff.wall_top_z }
  local cover_right = { kerb_back_x, kerb_back_y, ff.wall_top_z }
  local wall_left = { ff.head_back_x, ff.head_back_y, ff.wall_bottom_z }
  local wall_right = { kerb_back_x, kerb_back_y, ff.wall_bottom_z }

  local sheet = polyhedron {
    points = {
      { rbct_x, rbct_left_y, rbct_top_z }, -- 0
      { rbct_x, rbct_left_y, rbct_bottom_z }, -- 1
      { ff.head_front_x, cover_front_y, ff.wall_top_z }, -- 2
      { cover_back_x, cover_back_y, ff.wall_top_z }, -- 3
      cover_left, -- 4
      cover_right, -- 5
    },
    faces = {
      { 0, 4, 2 },
      { 1, 3, 5 },
      -- {5,4,0,1} is a warped quad. Splitting it 4-1 rather than on the
      -- fan diagonal is what the original produces; on a non-planar face
      -- the two choices enclose measurably different volume, so the split
      -- is written out rather than left to the tessellator.
      { 5, 4, 1 },
      { 4, 0, 1 },
      { 0, 2, 3, 1 },
      { 2, 4, 5, 3 },
    },
  }

  local skirt = polyhedron {
    points = {
      { ff.head_front_x, cover_front_y, ff.wall_top_z }, -- 0
      { cover_back_x, cover_back_y, ff.wall_top_z }, -- 1
      cover_left, -- 2
      cover_right, -- 3
      { ff.head_front_x, cover_front_y, ff.wall_bottom_z }, -- 4
      { cover_back_x, cover_back_y, ff.wall_bottom_z }, -- 5
      wall_left, -- 6
      wall_right, -- 7
    },
    faces = {
      { 0, 1, 3, 2 },
      { 0, 2, 6, 4 },
      { 1, 5, 7, 3 },
      { 2, 3, 7, 6 },
      { 1, 0, 4, 5 },
      { 5, 4, 6, 7 },
    },
  }

  return (sheet + skirt) - d455_removal()
end

function M.front_right_side()
  local cover_left = { kerb_back_x, -kerb_back_y, ff.wall_top_z }
  local cover_right = { ff.head_back_x, -ff.head_back_y, ff.wall_top_z }
  local wall_left = { kerb_back_x, -kerb_back_y, ff.wall_bottom_z }
  local wall_right = { ff.head_back_x, -ff.head_back_y, ff.wall_bottom_z }

  local sheet = polyhedron {
    points = {
      { rbct_x, rbct_right_y, rbct_top_z }, -- 0
      { rbct_x, rbct_right_y, rbct_bottom_z }, -- 1
      { ff.head_front_x, -cover_front_y, ff.wall_top_z }, -- 2
      { cover_back_x, -cover_back_y, ff.wall_top_z }, -- 3
      cover_left, -- 4
      cover_right, -- 5
    },
    faces = {
      { 0, 2, 5 },
      { 1, 4, 3 },
      -- Warped quad {5,4,1,0}, split 4-0 to match the original.
      { 5, 4, 0 },
      { 4, 1, 0 },
      { 0, 1, 3, 2 },
      { 2, 3, 4, 5 },
    },
  }

  local skirt = polyhedron {
    points = {
      { ff.head_front_x, -cover_front_y, ff.wall_top_z }, -- 0
      { cover_back_x, -cover_back_y, ff.wall_top_z }, -- 1
      cover_left, -- 2
      cover_right, -- 3
      { ff.head_front_x, -cover_front_y, ff.wall_bottom_z }, -- 4
      { cover_back_x, -cover_back_y, ff.wall_bottom_z }, -- 5
      wall_left, -- 6
      wall_right, -- 7
    },
    faces = {
      { 1, 0, 3, 2 },
      { 3, 0, 4, 7 },
      { 1, 2, 6, 5 },
      { 0, 1, 5, 4 },
      { 2, 3, 7, 6 },
      { 4, 5, 6, 7 },
    },
  }

  return (sheet + skirt) - usb_cutout() - d455_removal()
end

--------------------------------------------------------------------------
-- Rear side panels
--
-- These reach back along the tray's side wall and carry a triangular
-- gusset that fills the corner beside the camera bay.
--------------------------------------------------------------------------

local rear_wall_x = ff.side_wall_x - ff.side_wall_length / 2
-- Top of the tray's side wall: where the sloping sheet lands.
local rear_wall_z = ff.side_wall_z + ff.wall_height / 2
-- Mid-height of that wall: where the bolt-on lip sits.
local rear_lip_z = ff.height / 2 + ff.wall_height / 2

function M.back_left_side()
  local cover_left = { rear_wall_x, ff.width / 2, rear_wall_z }
  local cover_right = {
    rear_wall_x,
    ff.side_wall_left_y + ff.kerb_thickness / 2,
    rear_wall_z,
  }

  local sheet = polyhedron {
    points = {
      { rbct_x, rbct_left_y, rbct_top_z }, -- 0
      { rbct_x, rbct_left_y, rbct_bottom_z }, -- 1
      { ff.head_back_x, ff.head_back_y, ff.wall_top_z }, -- 2
      { kerb_back_x, kerb_back_y, ff.wall_top_z }, -- 3
      cover_left, -- 4
      cover_right, -- 5
    },
    faces = {
      { 0, 4, 2 },
      { 1, 3, 5 },
      { 0, 1, 5, 4 },
      -- Warped quad {2,3,1,0}, split 3-0 to match the original.
      { 2, 3, 0 },
      { 3, 1, 0 },
      { 2, 4, 5, 3 },
    },
  }

  -- The gusset beside the bay. Its inboard bottom corner is found by
  -- walking the panel's own y/z slope down from the flange.
  local corner_slope = (rbct_left_y - cover_left[2])
    / (rbct_top_z - cover_left[3])
  local fill_back_left_bottom_y = mount_left_top[2]
    + corner_slope * (mount_left_bottom[3] - cut_back_left_top[3])

  local gusset = polyhedron {
    points = {
      { mount_left_top[1], mount_left_top[2], cut_back_left_top[3] }, -- 0
      { mount_left_bottom[1], fill_back_left_bottom_y, mount_left_bottom[3] }, -- 1
      { mount_left_bottom[1], mount_left_bottom[2] - 3, mount_left_bottom[3] }, -- 2
      { cut_back_left_top[1], fill_back_left_bottom_y, mount_left_bottom[3] }, -- 3
      { cut_back_left_top[1], cut_back_left_top[2] - 3, mount_left_bottom[3] }, -- 4
      { cut_back_left_top[1], mount_left_top[2], cut_back_left_top[3] }, -- 5
    },
    faces = {
      { 0, 2, 1 },
      { 5, 3, 4 },
      { 0, 1, 3, 5 },
      -- Warped quad {0,5,4,2}, split 5-2 to match the original.
      { 0, 5, 2 },
      { 5, 4, 2 },
      { 1, 2, 4, 3 },
    },
  }

  local wall_length = ff.head_back_x - rear_wall_x
  local wall_width = cover_left[2] - cover_right[2]
  local wall_x = (ff.head_back_x + rear_wall_x) / 2
  local wall_y = (cover_left[2] + cover_right[2]) / 2

  local wall = cube {
    { wall_length, wall_width, ff.wall_height },
    center = true,
  }:translate(wall_x, wall_y, rear_lip_z)

  local screw = u.m2_5_flathead_screw(wall_width)
    :scale(1, 1.001, 1)
    :rotate(-90, 0, 0)

  return sheet
    + gusset
    + wall
    - screw:translate(ff.side_screw_front_x, wall_y, rear_lip_z)
    - screw:translate(ff.side_screw_back_x, wall_y, rear_lip_z)
    - d455_removal()
    - d455_head_bore():translate(d455_head_x, d455_screw_left_y, d455_screw_z)
end

function M.back_right_side()
  local cover_left = {
    rear_wall_x,
    ff.side_wall_right_y - ff.kerb_thickness / 2,
    rear_wall_z,
  }
  local cover_right = { rear_wall_x, -ff.width / 2, rear_wall_z }

  local sheet = polyhedron {
    points = {
      { rbct_x, rbct_right_y, rbct_top_z }, -- 0
      { rbct_x, rbct_right_y, rbct_bottom_z }, -- 1
      { kerb_back_x, -kerb_back_y, ff.wall_top_z }, -- 2
      { ff.head_back_x, -ff.head_back_y, ff.wall_top_z }, -- 3
      cover_left, -- 4
      cover_right, -- 5
    },
    faces = { { 0, 3, 5 }, { 1, 4, 2 }, { 1, 0, 5, 4 }, { 0, 1, 2, 3 }, { 3, 2, 4, 5 } },
  }

  local corner_slope = (rbct_right_y - cover_right[2])
    / (rbct_top_z - cover_right[3])
  local fill_back_right_bottom_y = mount_right_top[2]
    + corner_slope * (mount_right_bottom[3] - cut_back_right_top[3])

  local gusset = polyhedron {
    points = {
      { mount_right_top[1], mount_right_top[2], cut_back_right_top[3] }, -- 0
      { mount_right_bottom[1], fill_back_right_bottom_y, mount_right_bottom[3] }, -- 1
      { mount_right_bottom[1], mount_right_bottom[2] + 3, mount_right_bottom[3] }, -- 2
      { cut_back_right_top[1], fill_back_right_bottom_y, mount_right_bottom[3] }, -- 3
      { cut_back_right_top[1], cut_back_right_top[2] + 3, mount_right_bottom[3] }, -- 4
      { cut_back_right_top[1], mount_right_top[2], cut_back_right_top[3] }, -- 5
    },
    faces = {
      { 1, 3, 4, 2 },
      { 0, 1, 2 },
      { 5, 4, 3 },
      { 0, 5, 3, 1 },
      -- Warped quad {5,0,2,4}, split 0-4 to match the original.
      { 5, 0, 4 },
      { 0, 2, 4 },
    },
  }

  local wall_length = ff.head_back_x - rear_wall_x
  local wall_width = cover_left[2] - cover_right[2]
  local wall_x = (ff.head_back_x + rear_wall_x) / 2
  local wall_y = (cover_left[2] + cover_right[2]) / 2

  local wall = cube {
    { wall_length, wall_width, ff.wall_height },
    center = true,
  }:translate(wall_x, wall_y, rear_lip_z)

  local screw = u.m2_5_flathead_screw(wall_width)
    :scale(1, 1.001, 1)
    :rotate(90, 0, 0)

  return sheet
    + gusset
    + wall
    - screw:translate(ff.side_screw_front_x, wall_y, rear_lip_z)
    - screw:translate(ff.side_screw_back_x, wall_y, rear_lip_z)
    - usb_cutout()
    - d455_removal()
    - d455_head_bore():translate(d455_head_x, d455_screw_right_y, d455_screw_z)
end

return M
