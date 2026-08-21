-- The upper half of the car: the crossbar spine, the two electronics
-- trays it carries, the bodywork over them, and the payload itself.
--
-- Ported from the placement offsets in v3/scad/racecar_all.scad.
--
-- Everything here is positioned relative to the crossbar body, which is
-- the origin the original model hangs the whole upper structure from.

local palette = require("parts.palette")
local crossbar = require("parts.crossbar")
local support = require("parts.foundation_support")
local servo_cage = require("parts.servo_cage")
local back_foundation = require("parts.back_foundation")
local front_foundation = require("parts.front_foundation")
local back_cover = require("parts.back_cover")
local front_cover = require("parts.front_cover")
local electronics = require("parts.electronics")

local M = {}

-- Where the chassis hangs beneath the crossbar.
M.chassis_x = -9.0
M.chassis_y = 0.0
M.chassis_z = -60.0

local lower_right_height = servo_cage.height - 3.67
local lower_right_x = crossbar.body_screw_back_x
  - support.lower_right.length / 2
  - 26.5
local lower_right_y = crossbar.body_screw_y
  - support.lower_right.width / 2
  - 40.67
local lower_right_z = crossbar.height / 2 - lower_right_height / 2

-- Battery bays: one slung under the chassis, two in the rear tray.
local battery_bay_x = back_foundation.x
  - back_foundation.length / 2
  + 1.5
  + electronics.battery_length / 2
  + 1.5
local battery_bay_y = 0.5 * (48.75 + 3.0)
local battery_bay_z = back_foundation.z
  + back_foundation.height / 2
  + electronics.battery_height / 2

local jetson_x = back_foundation.x
  - back_cover.length / 2
  + electronics.jetson_length / 2
local jetson_z = back_foundation.z
  + back_cover.z
  - back_cover.height / 2
  - electronics.jetson_height / 2
  - electronics.jetson_support_height

-- The lidar sits with its own feet on the cover's mounting bosses.
local lidar_x = back_foundation.x
  - electronics.lidar_leg_front_x
  + back_cover.laser_leg_front_x
local lidar_y = -electronics.lidar_leg_front_left_y + back_cover.laser_leg_front_y
local lidar_z = back_foundation.z
  + back_cover.z
  + back_cover.height / 2
  + electronics.lidar_height / 2
  - 3

-- Both cameras bolt to the front of the plate behind the bay, on the
-- centre line of the camera bolt pattern.
local d435_x = front_foundation.x
  + front_cover.plate_front_x
  + electronics.d435_length / 2
local d435_y = front_foundation.y + front_cover.camera_mid_y
local d435_z = front_foundation.z + front_cover.camera_screw_z

local t265_x = front_foundation.x
  + front_cover.plate_front_x
  + electronics.t265_length / 2
local t265_z = front_foundation.z + front_cover.t265_screw_z

local servo_x = servo_cage.x
local servo_y = servo_cage.y
  - servo_cage.width / 2
  - electronics.servo_faceplate_y
  - electronics.servo_faceplate_width / 2
local servo_z = servo_cage.z
  - servo_cage.height / 2
  + electronics.servo_body_height / 2

--------------------------------------------------------------------------

function M.parts()
  local out = {}
  -- `look` names a role in the palette, which supplies the colour and,
  -- for some roles, a surface material.
  local function add(solid, look, name)
    out[#out + 1] = {
      solid = solid,
      color = palette.colors[look],
      material = palette.materials[look],
      name = name,
    }
  end

  -- Spine, plus the two full-height feet that stand it off the chassis.
  -- racecar_all.scad draws only the feet; the thin spine plate they bolt
  -- to is a real part, so it is included here.
  add(
    crossbar.body()
      + crossbar.upper_support():translate(
        crossbar.upper_x,
        0,
        crossbar.upper_z
      )
      + crossbar.bottom_support():translate(
        crossbar.bottom_x,
        0,
        crossbar.bottom_z
      ),
    "deck",
    "crossbar"
  )
  add(
    support.upper_left_support():translate(
      support.upper_left.x,
      support.upper_left.y,
      support.upper_left.z
    )
      + support.lower_left_support():translate(
        support.lower_left.x,
        support.lower_left.y,
        support.lower_left.z
      )
      + support
        .lower_right_support(lower_right_height)
        :translate(lower_right_x, lower_right_y, lower_right_z),
    "cover",
    "foundation-supports"
  )
  add(
    servo_cage.servo_cage():translate(servo_cage.x, servo_cage.y, servo_cage.z),
    "cover",
    "servo-cage"
  )

  -- Electronics trays
  add(
    back_foundation.back_foundation():translate(
      back_foundation.x,
      back_foundation.y,
      back_foundation.z
    ),
    "deck",
    "tray-rear"
  )
  add(
    front_foundation.front_foundation():translate(
      front_foundation.x,
      front_foundation.y,
      front_foundation.z
    ),
    "deck",
    "tray-front"
  )

  -- Rear bodywork
  local cover_origin = function(solid)
    return solid:translate(back_foundation.x, back_foundation.y, back_foundation.z)
  end

  add(
    cover_origin(back_cover.left_side() + back_cover.right_side()),
    "cover",
    "cover-rear-sides"
  )
  add(
    cover_origin(back_cover.top():translate(0, 0, back_cover.z)),
    "cover",
    "cover-rear-top"
  )

  -- Front bodywork
  local front_origin = function(solid)
    return solid:translate(
      front_foundation.x,
      front_foundation.y,
      front_foundation.z
    )
  end

  add(front_origin(front_cover.center()), "cover", "cover-front-center")
  add(
    front_origin(front_cover.front_left_side() + front_cover.front_right_side()),
    "cover",
    "cover-front-sides"
  )
  add(
    front_origin(front_cover.back_left_side() + front_cover.back_right_side()),
    "cover",
    "cover-front-rear-sides"
  )
  add(front_origin(front_cover.camera_plate()), "cover", "camera-plate")

  -- Payload
  add(
    electronics.servo_body():translate(servo_x, servo_y, servo_z),
    "servo",
    "servo"
  )
  add(
    (electronics.servo_arm() + electronics.servo_arm_link())
      :translate(servo_x, servo_y, servo_z),
    "link",
    "servo-linkage"
  )

  local battery = electronics.battery()
  add(
    battery:translate(
      M.chassis_x,
      M.chassis_y + electronics.battery_width / 2 + 5.0,
      M.chassis_z + 2.54 / 2 + electronics.battery_height / 2
    ),
    "battery",
    "battery-drive"
  )
  add(
    battery:translate(battery_bay_x, battery_bay_y, battery_bay_z)
      + battery:translate(battery_bay_x, -battery_bay_y, battery_bay_z),
    "battery",
    "battery-logic"
  )

  -- Mounted upside down under the cover.
  add(
    electronics.jetson_nano():rotate(180, 0, 0):translate(jetson_x, 0, jetson_z),
    "board",
    "jetson-nano"
  )

  add(
    (electronics.lidar_mount() + electronics.lidar_motor())
      :translate(lidar_x, lidar_y, lidar_z),
    "electronics",
    "lidar-mount"
  )
  add(
    electronics.lidar_top():translate(lidar_x, lidar_y, lidar_z),
    "lidar",
    "lidar-scanner"
  )

  -- The two RealSense cameras looking out of the front bay.
  add(
    electronics.d435():translate(d435_x, d435_y, d435_z),
    "electronics",
    "camera-d435"
  )
  add(
    electronics.d435_screen():translate(d435_x, d435_y, d435_z),
    "camera",
    "camera-d435-lens"
  )
  add(
    electronics.t265():translate(t265_x, d435_y, t265_z),
    "camera",
    "camera-t265"
  )

  return out
end

return M
