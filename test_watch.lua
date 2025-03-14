require("luacad")

-- Add a global counter to track executions
if not _G.counter then
  _G.counter = 0
end
_G.counter = _G.counter + 1

-- Create a variable definition using var
local x = var("x", _G.counter)

print("Execution #" .. _G.counter .. ", x = " .. x)

-- Create a simple model
model = cube { size = { 3, 3, 5 } }
model:export("test.scad")

-- Check the content of the SCAD file to verify no duplication
local f = io.open("test.scad", "r")
local content = f:read("*all")
f:close()

-- Count occurrences of "x = "
local count = 0
for _ in content:gmatch("x = ") do
  count = count + 1
end
print("Number of 'x = ' occurrences in test.scad:", count)
