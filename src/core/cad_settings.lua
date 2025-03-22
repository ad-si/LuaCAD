local openscad_settings_windows = {
  executable = [["C:\Program Files\OpenSCAD\openscad.com"]],
  scadfile = [[D:\Temp\lua_openscad_tmp.scad]],
  tmpfile = [[D:\Temp\lua_cad_tmp]],
  default_segments = 18,
  include = "",
}

local openscad_settings_macos = {
  executable = "/opt/homebrew/bin/openscad",
  scadfile = "/tmp/lua_openscad_tmp.scad",
  tmpfile = "/tmp/lua_cad_tmp",
  default_segments = 18,
  include = "",
}

local openscad_settings_linux = {
  executable = "/usr/bin/openscad",
  scadfile = "/tmp/lua_openscad_tmp.scad",
  tmpfile = "/tmp/lua_cad_tmp",
  default_segments = 18,
  include = "",
}

-- Return the correct settings for the current OS
local function get_openscad_settings()
  local osName = package.config:sub(1, 1) == "\\" and "Windows"
    or io.popen("uname"):read("*l")

  if osName and string.match(osName, "Windows") then
    return openscad_settings_windows
  elseif osName and string.match(osName, "Darwin") then
    return openscad_settings_macos
  elseif osName and string.match(osName, "Linux") then
    return openscad_settings_linux
  else
    print("Error: OS " .. osName .. " not supported")
    return nil
  end
end

return get_openscad_settings()
