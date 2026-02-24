local cad = cad or {}

function cad.is_bool(t)
  return type(t) == "boolean"
end

function cad.is_num(t)
  return type(t) == "number"
end

function cad.is_str(t)
  return type(t) == "string"
end

function cad.is_table(t)
  return type(t) == "table"
end

function cad.is_list(t)
  local count = 0
  for _ in pairs(t) do
    count = count + 1
  end
  return count == #t
end

function cad.is_func(t)
  return type(t) == "function"
end

function cad.concat(list_a, list_b)
  assert(is_list(list_a), "list_a is not a list")
  assert(is_list(list_b), "list_b is not a list")
  return table.move(list_b, 1, #list_b, #list_b + 1, list_a)
end

function cad.version()
  local file = io.open("versions.txt", "r")
  if file then
    print(file:read("*a"))
    file:close()
  else
    print("0.0.0")
  end
end
