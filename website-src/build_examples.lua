#!/usr/bin/env lua

-- build_examples.lua
-- Builds examples.html: each example's source, the OpenSCAD it exports to,
-- and a rendered preview, side by side. An example written in OpenSCAD rather
-- than Lua gets the same three columns; only the label and the highlighting of
-- the first one change.
--
-- Everything goes through the `luacad` CLI, so no OpenSCAD installation is
-- needed. Set LUACAD to point at a specific binary, e.g. when testing a build
-- that is not on PATH:
--
--   LUACAD=../target/debug/luacad lua build_examples.lua

-- Constants
local EXAMPLES_DIR = "../examples"
local WEBSITE_DIR = "../website"
local TEMPLATE_FILE = "example_template.html"
local HEADER_FILE = "examples_header.html"
local FOOTER_FILE = "examples_footer.html"
local OUTPUT_FILE = WEBSITE_DIR .. "/examples.html"
local IMAGES_DIR = WEBSITE_DIR .. "/images"
local SCAD_DIR = os.getenv("TMPDIR") or "/tmp"
local LUACAD = os.getenv("LUACAD") or "luacad"
-- A model built from hundreds of parts exports thousands of lines of SCAD,
-- which is neither readable on the page nor cheap to syntax-highlight.
local MAX_SCAD_LINES = 120

-- Names the generic capitalisation gets wrong.
local DISPLAY_NAMES = {
  bosl_demo = "BOSL2 Demo",
  bosl_shapes_demo = "BOSL2 Shapes",
  csg = "CSG",
  csg_modules = "CSG Modules",
  literal_openscad = "Literal OpenSCAD",
  mushr_racecar = "MuSHR Racecar",
  text = "Text",
  tostring_demo = "Tostring Demo",
}

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

-- Prepare code for display (trim and escape HTML special characters)
local function prepare_code(content)
  content = trim(content)
  return content
    :gsub("&", "&amp;")
    :gsub("<", "&lt;")
    :gsub(">", "&gt;")
    :gsub('"', "&quot;")
end

-- Keep long SCAD exports readable, saying plainly what was left out
local function truncate_scad(content, name, path)
  local lines = {}
  -- `[^\n]*` also matches the empty string after every newline, which counts
  -- each line twice; anchor on the separator instead.
  for line in (content:gsub("\n$", "") .. "\n"):gmatch("(.-)\n") do
    lines[#lines + 1] = line
  end

  if #lines <= MAX_SCAD_LINES then
    return content
  end

  local kept = {}
  for i = 1, MAX_SCAD_LINES do
    kept[i] = lines[i]
  end
  kept[#kept + 1] = ""
  kept[#kept + 1] = string.format(
    "// … %d more lines. Run `luacad convert %s %s.scad` for all of it.",
    #lines - MAX_SCAD_LINES,
    (path:gsub("^%.%./", "")),
    name
  )

  print(
    string.format(
      "ℹ️  Truncated %s SCAD to %d of %d lines",
      name,
      MAX_SCAD_LINES,
      #lines
    )
  )

  return table.concat(kept, "\n")
end

-- Format example name for display (underscores to spaces, capitalize words)
local function format_example_name(name)
  if DISPLAY_NAMES[name] then
    return DISPLAY_NAMES[name]
  end

  name = name:gsub("[_%-]", " ")

  name = name:gsub("(%w)([%w]*)", function(first, rest)
    return first:upper() .. rest:lower()
  end)

  return name
end

-- Run a command, discarding its output, and report whether it succeeded
local function run(cmd)
  local ok = os.execute(cmd .. " > /dev/null 2>&1")
  -- Lua 5.1 returns an exit code, 5.2+ returns a boolean first
  return ok == true or ok == 0
end

-- Every example: the plain `*.lua` and `*.scad` files, plus one entry point
-- per subdirectory for the examples that are split across several files.
local function find_examples()
  local examples = {}

  local files = io.popen("ls " .. EXAMPLES_DIR .. "/*.lua " .. EXAMPLES_DIR
    .. "/*.scad 2>/dev/null")
  for path in files:read("*a"):gmatch("[^\n]+") do
    examples[#examples + 1] = {
      name = path:match("([^/]+)%.%w+$"),
      path = path,
    }
  end
  files:close()

  local dirs = io.popen("ls -d " .. EXAMPLES_DIR .. "/*/ 2>/dev/null")
  for dir in dirs:read("*a"):gmatch("[^\n]+") do
    local name = dir:match("([^/]+)/$")
    -- The entry point is the single .lua or .scad directly inside the
    -- directory; anything deeper is a module it includes or requires.
    local entries =
      io.popen("ls " .. dir .. "*.lua " .. dir .. "*.scad 2>/dev/null")
    local entry = entries:read("*a"):match("[^\n]+")
    entries:close()
    if entry then
      examples[#examples + 1] = { name = name, path = entry }
    else
      print("⚠️  No entry point found in " .. dir)
    end
  end
  dirs:close()

  table.sort(examples, function(a, b)
    return a.name < b.name
  end)

  return examples
end

-- Process a single example
local function process_example(example)
  print("⏳ Processing " .. example.path)

  local display_name = format_example_name(example.name)

  local template, err = read_file(TEMPLATE_FILE)
  if not template then
    print("Error reading template: " .. err)
    return false
  end

  template = replace_placeholder(template, "EXAMPLE_DISPLAY_NAME", display_name)
  template = replace_placeholder(template, "EXAMPLE_FILENAME", example.name)

  -- An OpenSCAD example is its own first column, so the second one holds what
  -- `convert` makes of it rather than a translation into another language.
  local is_scad = example.path:match("%.scad$") ~= nil
  template = replace_placeholder(
    template,
    "SOURCE_TITLE",
    is_scad and "OpenSCAD" or "LuaCAD"
  )
  template = replace_placeholder(
    template,
    "SOURCE_LANGUAGE",
    is_scad and "openscad" or "lua"
  )
  template = replace_placeholder(
    template,
    "EXPORT_TITLE",
    is_scad and "Flattened OpenSCAD" or "Generated OpenSCAD"
  )

  local source_content, source_err = read_file(example.path)
  if not source_content then
    print("Error reading source file: " .. source_err)
    return false
  end
  template =
    replace_placeholder(template, "EXAMPLE_CODE", prepare_code(source_content))

  -- Export the SCAD the script produces, for the second column.
  local scad_file = SCAD_DIR .. "/" .. example.name .. ".scad"
  local scad_content
  if
    run(
      string.format("%s convert %s %s", LUACAD, example.path, scad_file)
    )
  then
    scad_content = read_file(scad_file)
    os.remove(scad_file)
  end

  if scad_content then
    template = replace_placeholder(
      template,
      "OPENSCAD_CODE",
      prepare_code(truncate_scad(scad_content, example.name, example.path))
    )
  else
    template = replace_placeholder(
      template,
      "OPENSCAD_CODE",
      "OpenSCAD code not available"
    )
    print("⚠️  Could not export SCAD for " .. example.name)
  end

  -- Render the preview. A script that only OpenSCAD can build has no mesh to
  -- render; the template falls back to a placeholder image for those.
  local png_file = IMAGES_DIR .. "/" .. example.name .. ".png"
  print("⏳ Rendering " .. png_file)
  if run(string.format("%s render %s %s", LUACAD, example.path, png_file)) then
    print("✅ Generated " .. png_file)
  else
    print("⚠️  Could not render " .. example.name .. ", using placeholder")
  end

  local status, append_err = append_file(OUTPUT_FILE, template)
  if not status then
    print("Error appending template to output: " .. append_err)
    return false
  end

  return true
end

-- Main function
local function main()
  ensure_dir(IMAGES_DIR)

  if not run(LUACAD .. " --version") then
    print("❌ Could not run `" .. LUACAD .. "`. Install it with `make install`")
    print("   or point LUACAD at a built binary.")
    return false
  end

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

  local failed = 0
  for _, example in ipairs(find_examples()) do
    if not process_example(example) then
      failed = failed + 1
      print("Error processing example: " .. example.path)
    end
  end

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

  if failed > 0 then
    print("⚠️  " .. failed .. " example(s) failed")
    return false
  end

  print("✅ Generated " .. OUTPUT_FILE .. " successfully")
  return true
end

os.exit(main() and 0 or 1)
