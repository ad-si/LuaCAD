-- Colours for the assembled car.
--
-- The original OpenSCAD model is monochrome apart from a single call on
-- the D435's lens, so the whole scheme here is invented. Two rules:
--
--   * Structure and mechanism are coloured as materials -- dark
--     mouldings, bare aluminium linkage, black rubber -- with the
--     springs warm so the four corners of suspension read at a glance.
--   * The payload is coloured by function rather than by what the real
--     hardware looks like -- each sensor gets its own hue so it is
--     identifiable at a glance. The real lidar and cameras are both
--     dark plastic.
--
-- Built on the Nord palette -- https://www.nordtheme.com -- which supplies
-- the structural greys, the warm spring, the green scanner and the blue
-- lenses. The other eight are mixed to fill gaps Nord does not cover:
-- rubber, aluminium, PCB green, battery and the darker housings.

return {
  -- Flat plate stock: chassis, crossbar, both electronics trays
  deck = "#2e3440",
  gearbox = "#434c5e", -- moulded nylon housings
  -- Bodywork, brackets and the motor cover
  cover = "#3b4252",
  bumper = "#4c566a", -- front and rear bumpers

  tire = "#191a1d", -- rubber
  hub = "#d8dee9", -- printed wheel centre

  link = "#9aa4b2", -- aluminium suspension linkage and uprights
  shaft = "#7b8494", -- drive shafts
  damper = "#c8ced8", -- polished shock body
  spring = "#d08770", -- warm, so the suspension stands out

  electronics = "#2b2b2f", -- sensor housings and mounts
  board = "#3d6b46", -- PCB green, much darker than the scanner's
  lidar = "#a3be8c", -- the spinning scanner
  camera = "#5e81ac", -- the two RealSense lenses
  battery = "#8fa1b3",
  servo = "#2e3338",
}
