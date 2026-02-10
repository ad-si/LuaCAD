use csgrs::mesh::Mesh as CsgMesh;
use mlua::{Lua, Result as LuaResult, Value as LuaValue};

use crate::app::AppState;
use crate::geometry::CsgGeometry;

fn lua_val_to_f32(v: &LuaValue) -> Option<f32> {
  match v {
    LuaValue::Number(n) => Some(*n as f32),
    LuaValue::Integer(n) => Some(*n as f32),
    _ => None,
  }
}

/// Parse cube() arguments: cube(size), cube(w, d, h), or cube({w, d, h})
fn parse_cube_args(args: &mlua::MultiValue) -> mlua::Result<(f32, f32, f32)> {
  if args.is_empty() {
    return Err(mlua::Error::RuntimeError(
      "cube() requires at least 1 argument".to_string(),
    ));
  }

  let first = &args[0];

  // Table form: cube({w, d, h})
  if let LuaValue::Table(t) = first {
    let w: f32 = t.get::<f32>(1).unwrap_or(1.0);
    let d: f32 = t.get::<f32>(2).unwrap_or(1.0);
    let h: f32 = t.get::<f32>(3).unwrap_or(1.0);
    return Ok((w, d, h));
  }

  // Number forms
  let s = lua_val_to_f32(first).ok_or_else(|| {
    mlua::Error::RuntimeError(
      "cube() argument must be a number, three numbers, or {w, d, h} table"
        .to_string(),
    )
  })?;

  // cube(w, d, h) — three separate number args
  if args.len() >= 3 {
    let d = lua_val_to_f32(&args[1]).unwrap_or(s);
    let h = lua_val_to_f32(&args[2]).unwrap_or(s);
    return Ok((s, d, h));
  }

  // cube(size) — uniform
  Ok((s, s, s))
}

impl AppState {
  pub fn execute_lua_code(&mut self) {
    self.lua_error = None;
    self.geometries.clear();

    let lua = Lua::new();
    let collector =
      std::rc::Rc::new(std::cell::RefCell::new(Vec::<CsgGeometry>::new()));

    let result: LuaResult<mlua::MultiValue> = (|| {
      let print_fn =
        lua.create_function(|_, args: mlua::Variadic<mlua::Value>| {
          let output = args
            .iter()
            .map(|v| format!("{v:?}"))
            .collect::<Vec<_>>()
            .join("\t");
          println!("Lua output: {output}");
          Ok(())
        })?;
      lua.globals().set("print", print_fn)?;

      // cube() — returns CsgGeometry userdata
      let cube_fn = lua.create_function(|_, args: mlua::MultiValue| {
        let (w, d, h) = parse_cube_args(&args)?;
        // cuboid() places one corner at origin (0,0,0)
        let mesh = CsgMesh::<()>::cuboid(w, d, h, None);
        Ok(CsgGeometry { mesh })
      })?;
      lua.globals().set("cube", cube_fn)?;

      // sphere() — returns CsgGeometry userdata
      let sphere_fn =
        lua.create_function(|_, (radius, segments): (f32, Option<u32>)| {
          let segs = segments.unwrap_or(16);
          let mesh =
            CsgMesh::<()>::sphere(radius, segs as usize, segs as usize, None);
          Ok(CsgGeometry { mesh })
        })?;
      lua.globals().set("sphere", sphere_fn)?;

      // cylinder() — returns CsgGeometry userdata
      let cylinder_fn = lua.create_function(
        |_, (radius, height, segments): (f32, f32, Option<u32>)| {
          let segs = segments.unwrap_or(16);
          let mesh =
            CsgMesh::<()>::cylinder(radius, height, segs as usize, None);
          Ok(CsgGeometry { mesh })
        },
      )?;
      lua.globals().set("cylinder", cylinder_fn)?;

      // render() — adds geometry to scene
      let collector_clone = collector.clone();
      let render_fn =
        lua.create_function(move |_, ud: mlua::AnyUserData| {
          let geom = ud.borrow::<CsgGeometry>()?.clone();
          collector_clone.borrow_mut().push(geom);
          Ok(())
        })?;
      lua.globals().set("render", render_fn)?;

      lua.load(&self.text_content).eval::<mlua::MultiValue>()
    })();

    match result {
      Ok(returns) => {
        // Auto-render any CsgGeometry returned from top-level
        for val in returns.iter() {
          if let LuaValue::UserData(ud) = val
            && let Ok(geom) = ud.borrow::<CsgGeometry>()
          {
            collector.borrow_mut().push(geom.clone());
          }
        }
        self.geometries = collector.borrow().clone();
        if self.geometries.is_empty() {
          self.lua_error = Some(
            "No geometry to render. Use render(obj) or return a geometry object."
              .to_string(),
          );
        }
      }
      Err(e) => {
        self.lua_error = Some(format!("Lua error: {e}"));
      }
    }

    self.scene_dirty = true;
  }
}
