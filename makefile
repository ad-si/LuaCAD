.PHONY: help
help: makefile
	@tail -n +4 makefile | grep ".PHONY"


TEST_FILES = $(wildcard test_*.lua)


.PHONY: test
test:
	mkdir -p temp
	@for file in $(TEST_FILES); do \
		echo "🎬 Running $$file"; \
		lua $$file; \
		echo ""; \
	done


.PHONY: test-single
test-single:
	@if [ -z "$(file)" ]; then \
		echo "Error: Please specify a test file with 'make test-single file=test_file.lua'"; \
		exit 1; \
	fi
	mkdir -p temp
	@echo "🎬 Running $(file)"
	@lua $(file)


.PHONY: benchmark
benchmark:
	hyperfine \
		'lua test_diabolo_cylindric.lua' \
		'luajit test_diabolo_cylindric.lua'


.PHONY: clean
clean:
	rm -rf temp
