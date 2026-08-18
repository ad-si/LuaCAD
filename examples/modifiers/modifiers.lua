-- OpenSCAD modifier characters in LuaCAD:
--   *  disable      s(obj)  /  obj:skip()
--   !  show only    o(obj)  /  obj:only()
--   #  debug        d(obj)  /  obj:debug()
--   %  background   t(obj)  /  obj:transparent()

-- `#` debug: the subtracted cylinder still cuts the hole,
-- and is additionally shown as a translucent red highlight.
local block = cube({ 20, 20, 10 })
local hole = cylinder({ h = 12, r = 4 }):translate(10, 10, -1)
render(block - d(hole))

-- `%` background: excluded from CSG, drawn as a translucent gray ghost.
local ghost = sphere({ r = 6 }):translate(-12, 10, 5)
render(t(ghost))

-- `*` skip: this cube is not rendered at all.
local hidden = cube({ 10, 10, 10 }):translate(25, 0, 0)
render(s(hidden))

-- Regular object for comparison.
render(sphere({ r = 5 }):translate(10, 30, 5))

-- `!` show only: uncomment to render ONLY this cylinder
-- and hide everything else.
-- render(o(cylinder({ h = 15, r = 3 }):translate(10, -12, 0)))
