.PHONY: help
help: makefile
	@tail -n +4 makefile | grep ".PHONY"


TEST_FILES = $(wildcard tests/test_*.lua)


.PHONY: test
test:
	mkdir -p tests/temp
	lua tests/run_tests.lua
	@echo "📋 Running example files"
	@for file in examples/*.lua; do \
		echo "⏳ Running $$file"; \
		luajit $$file; \
	done


.PHONY: test-single
test-single:
	@if [ -z "$(file)" ]; then \
		echo "Error: Please specify a test file with 'make test-single file=test_file.lua'"; \
		exit 1; \
	fi
	mkdir -p tests/temp
	@echo "🎬 Running $(file)"
	@lua $(file)


.PHONY: benchmark
benchmark:
	hyperfine \
		'lua tests/test_diabolo_cylindric.lua' \
		'luajit tests/test_diabolo_cylindric.lua'


.PHONY: clean
clean:
	rm -rf tests/temp
	rm -f examples/*.scad


.PHONY: fmt
fmt:
	@echo "🎨 Formatting Lua code with StyLua"
	@stylua . luacad.lua bin/luacad
