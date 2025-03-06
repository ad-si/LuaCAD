local function get_script_path()
  -- Get the path of the current script
  local source = debug.getinfo(1).source

  -- If the source starts with @, remove it (standard for files)
  if source:sub(1, 1) == "@" then
    source = source:sub(2)
  end

  print("Raw source: " .. source)

  -- Get the directory part of the path
  local directory

  if source:match("^/") then
    -- Absolute path
    directory = source:match("(.*/)")
    print("Absolute path detected")
  else
    -- Relative path, which might be relative to current directory
    local abspath
    -- Try to convert to absolute path
    if source:find("/") then
      -- Path has directory components
      directory = source:match("^(.-)[^/]+$")
      print("Directory from relative: " .. (directory or "nil"))
    else
      -- Just a filename, use current directory
      directory = "./"
      print("Using current directory")
    end
  end

  print("Directory: " .. (directory or "nil"))
  return directory
end

print("Script path: " .. (get_script_path() or "unknown"))
