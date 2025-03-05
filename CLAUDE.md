# LuaSCAD Development Guide

## Build & Test Commands
- Run all tests: `make test`
- Run single test: `make test-single file=tests/test_file.lua` (e.g., `make test-single file=tests/test_cube.lua`)
- Clean temp files: `make clean`
- Benchmark Lua vs LuaJIT: `make benchmark`

## Testing Guidelines
- All tests use the luaunit framework
- Test classes should be named `Test[Feature]` (e.g., `TestCube`)
- Test methods should be named `test[Functionality]` (e.g., `testCubeCreation`)
- Use `setUp()` method to initialize test environment
- Include file existence assertions to validate output generation
- Each test file should end with `if not ... then os.exit(luaunit.LuaUnit.run()) end`

## Code Style Guidelines
- Indentation: 2 spaces (no tabs)
- Function naming: `lowerCamelCase` for methods, `snake_case` for local functions
- Object creation: Use `cad.object()` for new CAD objects
- Object modification: Use `object:method()` to modify existing objects
- Modules: Use `require "lib_module"` for imports (no parentheses)
- Model exports: Place generated files in the `temp/` directory
- Error handling: Use `assert()` for validation, provide descriptive error messages

## Project Architecture
- Library modules are prefixed with `lib_`
- Test files are prefixed with `test_` and stored in the `tests/` directory
- Template files are prefixed with `template_`
- CAD objects support the `+` operator for combining models