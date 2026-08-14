//! Report which `bosl.*` names LuaCAD builds itself and which still hand off
//! to OpenSCAD. Run with `--nocapture` to see the list.

#[test]
fn report_bosl_coverage() {
  let lua = mlua::Lua::new();
  luacad::bosl::register_bosl(&lua).unwrap();
  let names: Vec<String> = lua
    .load(
      "local out = {}
       for k, v in pairs(bosl) do
         if type(v) == 'function' then out[#out+1] = k end
       end
       table.sort(out)
       return out",
    )
    .eval()
    .unwrap();

  let mut shapes = Vec::new();
  let mut values = Vec::new();
  let mut pending = Vec::new();
  for name in &names {
    if luacad::bosl::builds_natively(name) {
      shapes.push(name.clone());
      continue;
    }
    // A function that still goes through OpenSCAD returns a geometry object
    // wrapping the call; anything else computes its answer here.
    let returns_geometry: bool = lua
      .load(format!(
        "local ok, v = pcall(bosl['{name}'])
         return ok and type(v) == 'userdata'"
      ))
      .eval()
      .unwrap_or(false);
    if returns_geometry {
      pending.push(name.clone());
    } else {
      values.push(name.clone());
    }
  }

  println!("bosl.* functions: {}", names.len());
  println!("  shapes built natively:       {}", shapes.len());
  println!("  functions computing a value: {}", values.len());
  println!("  still OpenSCAD-only:         {}", pending.len());
  println!("\nstill OpenSCAD-only:\n{}", pending.join(" "));

  assert!(
    !shapes.is_empty() && !values.is_empty(),
    "both kinds of native implementation should be present"
  );
}
