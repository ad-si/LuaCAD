#!/usr/bin/env lua

-- build_examples.lua
-- Script to build examples.html with LuaCAD and OpenSCAD code side by side

-- Constants
local EXAMPLES_DIR = "../examples"
local WEBSITE_DIR = "../website"
local TEMPLATE_FILE = "example_template.html"
local HEADER_FILE = "examples_header.html"
local FOOTER_FILE = "examples_footer.html"
local OUTPUT_FILE = WEBSITE_DIR .. "/examples.html"

-- Create directory if it doesn't exist
local function ensure_dir(path)
  os.execute("mkdir -p " .. path)
end

-- Read a file's contents
local function read_file(path)
  local file = io.open(path, "r")
  if not file then
    return nil, "Could not open file: " .. path
  end
  local content = file:read("*a")
  file:close()
  return content
end

-- Write content to a file
local function write_file(path, content)
  local file = io.open(path, "w")
  if not file then
    return nil, "Could not open file for writing: " .. path
  end
  file:write(content)
  file:close()
  return true
end

-- Append content to a file
local function append_file(path, content)
  local file = io.open(path, "a")
  if not file then
    return nil, "Could not open file for appending: " .. path
  end
  file:write(content)
  file:close()
  return true
end

-- Replace placeholders in content
local function replace_placeholder(content, placeholder, replacement)
  local pattern = placeholder:gsub("([%%%-%.[%]%(%)%+%*%?%^%$])", "%%%1")
  return (content:gsub(pattern, function()
    return replacement
  end))
end

-- Trim string (remove leading/trailing whitespace)
local function trim(s)
  return s:match("^%s*(.-)%s*$")
end

-- Prepare Lua code for display (trim and escape HTML special characters)
local function prepare_lua(content)
  content = trim(content)
  return content
    :gsub("&", "&amp;")
    :gsub("<", "&lt;")
    :gsub(">", "&gt;")
    :gsub('"', "&quot;")
end

-- Prepare OpenSCAD code for display (trim and escape HTML special characters)
local function prepare_scad(content)
  content = trim(content)
  return content
    :gsub("&", "&amp;")
    :gsub("<", "&lt;")
    :gsub(">", "&gt;")
    :gsub('"', "&quot;")
end

-- Format example name for display (convert underscores and dashes to spaces, capitalize words)
local function format_example_name(name)
  -- Replace underscores and dashes with spaces
  name = name:gsub("[_%-]", " ")

  -- Capitalize each word
  name = name:gsub("(%w)([%w]*)", function(first, rest)
    return first:upper() .. rest:lower()
  end)

  return name
end

-- Process a single example
local function process_example(lua_file)
  print("⏳ Processing " .. lua_file)

  -- Extract example name
  local example_name = lua_file:match("([^/]+)%.lua$")
  local display_name = format_example_name(example_name)

  -- Run the Lua file to generate OpenSCAD output
  os.execute("luajit " .. lua_file .. " > /dev/null 2>&1")

  -- Read template
  local template, err = read_file(TEMPLATE_FILE)
  if not template then
    print("Error reading template: " .. err)
    return false
  end

  -- Replace name and description
  template = replace_placeholder(template, "EXAMPLE_DISPLAY_NAME", display_name)
  template = replace_placeholder(template, "EXAMPLE_FILENAME", example_name)
  template = replace_placeholder(
    template,
    "EXAMPLE_DESCRIPTION",
    "Example demonstrating " .. display_name .. " functionality."
  )

  -- Read and prepare Lua code
  local lua_content, lua_err = read_file(lua_file)
  if not lua_content then
    print("Error reading Lua file: " .. lua_err)
    return false
  end

  local prepared_lua = prepare_lua(lua_content)
  template = replace_placeholder(template, "EXAMPLE_CODE", prepared_lua)

  -- Check for and process OpenSCAD file
  local scad_file = EXAMPLES_DIR .. "/" .. example_name .. ".scad"
  local scad_content, scad_err = read_file(scad_file)

  if scad_content then
    local prepared_scad = prepare_scad(scad_content)
    template = replace_placeholder(template, "OPENSCAD_CODE", prepared_scad)
  else
    template = replace_placeholder(
      template,
      "OPENSCAD_CODE",
      "OpenSCAD code not available"
    )
  end

  -- Append to output file
  local status, err = append_file(OUTPUT_FILE, template)
  if not status then
    print("Error appending template to output: " .. err)
    return false
  end

  return true
end

-- Main function
local function main()
  -- Create necessary directories
  ensure_dir(WEBSITE_DIR .. "/images")

  -- Initialize output file with header
  local header, header_err = read_file(HEADER_FILE)
  if not header then
    print("Error reading header: " .. header_err)
    return false
  end

  local status, out_err = write_file(OUTPUT_FILE, header)
  if not status then
    print("Error writing header to output: " .. out_err)
    return false
  end

  -- Process all example files
  local cmd = "ls " .. EXAMPLES_DIR .. "/*.lua"
  local handle = io.popen(cmd)
  local result = handle:read("*a")
  handle:close()

  for lua_file in result:gmatch("[^\n]+") do
    local success = process_example(lua_file)
    if not success then
      print("Error processing example: " .. lua_file)
    end
  end

  -- Append footer
  local footer, footer_err = read_file(FOOTER_FILE)
  if not footer then
    print("Error reading footer: " .. footer_err)
    return false
  end

  status, out_err = append_file(OUTPUT_FILE, footer)
  if not status then
    print("Error appending footer to output: " .. out_err)
    return false
  end

  print("✅ Generated " .. OUTPUT_FILE .. " successfully")
  return true
end

-- Run the script
main()
