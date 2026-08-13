-- Coil-over shock absorber.
--
-- Ported from chassis/racecar_chassis_shock_tower.scad.
--
-- Built standing on the Z axis and centred on the origin; the assembly
-- tilts it into place. A ball-joint connector caps each end, and the
-- spring is a real helix wound around the damper body.

local u = require("parts.utils")

local M = {}

M.length = 21.0
M.connector_length = 13.45

local connector_width = 6.00

--------------------------------------------------------------------------
-- End connector
--
-- Drawn twice per end: once solid to subtract the mount's bore, once
-- pierced to leave the eye open.
--------------------------------------------------------------------------

local function connector(include_cutout)
  local base_height = 3.0
  local base_radius = connector_width / 2
  local base_x = base_height / 2 - M.connector_length / 2

  local head_height = 5.5
  local head_radius = 4.5 / 2
  local head_x = -head_height / 2 + M.connector_length / 2

  local body_height = M.connector_length - head_height - base_height
  local body_x = head_x - head_height / 2 - body_height / 2

  local solid = u.cyl(base_height, base_radius)
    :rotate(0, 90, 0)
    :translate(base_x, 0, 0)
    + u.cyl(head_height, head_radius)
      :rotate(0, 90, 0)
      :translate(head_x, 0, 0)
    + u.cyl(body_height, base_radius, head_radius)
      :rotate(0, 90, 0)
      :translate(body_x, 0, 0)

  if not include_cutout then
    return solid
  end

  return solid
    - u.cyl(M.connector_length, head_radius - 0.8):rotate(0, 90, 0)
end

--------------------------------------------------------------------------
-- Shock tower
--------------------------------------------------------------------------

-- The damper on its own, without the spring wound around it. Kept separate
-- so the assembly can paint the spring a different colour.
function M.damper(height)
  height = height or 102.3

  local top_mount_height = 4.15
  local top_mount_radius = 13.15 / 2
  local top_mount_z = height / 2 - top_mount_radius

  local top_cube_height = 6.75
  local top_cube_z = top_mount_z - top_cube_height / 2

  local top_connector_x = top_mount_height / 2 - M.connector_length / 2 + 0.01

  local top_cap_height = 8.5
  local top_cap_diameter = 19.25
  local top_cap_z = top_cube_z - top_cube_height / 2 - top_cap_height / 2

  local bottom_mount_height = 3.4
  local bottom_mount_radius = 8.6 / 2
  local bottom_mount_z = -height / 2 + bottom_mount_radius

  local bottom_connector_x = bottom_mount_height / 2
    - M.connector_length / 2
    + 0.01

  local bottom_cube_height = 10.15
  local bottom_cube_z = bottom_mount_z + bottom_cube_height / 2

  local bottom_cap_height = 11.0
  local bottom_cap_bottom_radius = 9.75 / 2
  local bottom_cap_z = bottom_cube_z + bottom_cube_height / 2 + bottom_cap_height / 2

  local body_height = (top_cap_z - top_cap_height / 2)
    - (bottom_cap_z + bottom_cap_height / 2)
  local body_radius = 16.0 / 2
  local body_z = bottom_cap_z + bottom_cap_height / 2 + body_height / 2

  -- Upper eye: the mount is bored by the connector's outer profile, then
  -- the pierced connector is dropped back into the hole.
  local top_mount = (
    u.cyl(top_mount_height, top_mount_radius):rotate(0, 90, 0)
      :translate(0, 0, top_mount_z)
    + cube {
      { top_mount_height, 2 * top_mount_radius, top_cube_height },
      center = true,
    }:translate(0, 0, top_cube_z)
  ) - connector(false):translate(top_connector_x, 0, top_mount_z)

  local bottom_mount = (
    u.cyl(bottom_mount_height, bottom_mount_radius):rotate(0, 90, 0)
      :translate(0, 0, bottom_mount_z)
    + cube {
      { bottom_mount_height, 2 * bottom_mount_radius, bottom_cube_height },
      center = true,
    }:translate(0, 0, bottom_cube_z)
  ) - connector(false):translate(bottom_connector_x, 0, bottom_mount_z)

  return top_mount
    + connector(true):translate(top_connector_x, 0, top_mount_z)
    + u.hexagon(top_cap_height, top_cap_diameter):translate(0, 0, top_cap_z)
    + bottom_mount
    + connector(true):translate(bottom_connector_x, 0, bottom_mount_z)
    + u.cyl(bottom_cap_height, bottom_cap_bottom_radius, M.length / 2)
      :translate(0, 0, bottom_cap_z)
    + u.cyl(body_height, body_radius):translate(0, 0, body_z)
end

-- The coil wound around the damper, in its own right so it can be
-- coloured separately. Geometry is identical to the original's inline coil.
function M.spring(height)
  height = height or 102.3

  -- Recomputed rather than shared: these follow the same chain of offsets
  -- the damper uses, and keeping them local keeps each function readable.
  local top_mount_radius = 13.15 / 2
  local top_cap_z = height / 2
    - top_mount_radius
    - 6.75
    - 8.5 / 2
  local bottom_cap_z = -height / 2 + 8.6 / 2 + 10.15 + 11.0 / 2

  local body_height = (top_cap_z - 8.5 / 2) - (bottom_cap_z + 11.0 / 2)
  local body_z = bottom_cap_z + 11.0 / 2 + body_height / 2

  return u.comp_spring({
    M.length, -- outer diameter
    1.43, -- wire gauge
    body_height, -- free length
    7, -- coils
  }):translate(0, 0, body_z - body_height / 2)
end

function M.shock_tower(height)
  return M.damper(height) + M.spring(height)
end

return M
