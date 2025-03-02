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
	done


.PHONY: benchmark
benchmark:
	hyperfine \
		'lua test_diabolo_cylindric.lua' \
		'luajit test_diabolo_cylindric.lua'


.PHONY: clean
clean:
	rm -rf temp
