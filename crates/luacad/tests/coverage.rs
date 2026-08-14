//! Check that every `bosl.*` name is built or computed by LuaCAD itself
//! rather than handed off to OpenSCAD. Run with `--nocapture` for the counts.

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

  let pending = luacad::bosl::openscad_only_names();
  let native: Vec<&String> =
    names.iter().filter(|n| !pending.contains(n)).collect();

  println!("bosl.* functions:    {}", names.len());
  println!("  built or computed: {}", native.len());
  println!("  still OpenSCAD-only: {}", pending.len());

  assert!(!native.is_empty(), "some names should be native");
  assert!(
    pending.is_empty(),
    "{} bosl.* name{} still need OpenSCAD:\n{}",
    pending.len(),
    if pending.len() == 1 { "" } else { "s" },
    pending.join(" "),
  );
}
