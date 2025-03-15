.PHONY: help
help: makefile
	@tail -n +4 makefile | grep ".PHONY" | cut -d ":" -f2 | sort


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


website/examples.html: website-src/examples_header.html website-src/example_template.html website-src/examples_footer.html examples/*.lua
	@echo "🌐 Generating examples.html from examples directory"
	@mkdir -p website/images
	@cp website-src/examples_header.html website/examples.html
	@for file in examples/*.lua; do \
		example_name=$$(basename $$file .lua); \
		echo "⏳ Processing $$file"; \
		cp website-src/example_template.html temp_example.html; \
		sed -i '' "s/EXAMPLE_NAME/$$example_name/g" temp_example.html; \
		sed -i '' "s/EXAMPLE_DESCRIPTION/Example demonstrating $$example_name functionality./g" temp_example.html; \
		code=$$(cat $$file | sed -e 's/&/\\&amp;/g' -e 's/</\\&lt;/g' -e 's/>/\\&gt;/g' -e 's/"/\\"/g' \
			-e 's/\b\(require\|function\|local\|return\|end\|if\|then\|else\|for\|do\)\b/<span class="keyword">\\1<\/span>/g' \
			-e 's/\b\(cube\|sphere\|cylinder\|translate\|rotate\|export\)\b/<span class="function">\\1<\/span>/g' \
			-e 's/\(--.*\)/<span class="comment">\\1<\/span>/g'); \
		perl -pi -e "s|EXAMPLE_CODE|$$code|" temp_example.html; \
		cat temp_example.html >> website/examples.html; \
		rm temp_example.html; \
	done
	@cat website-src/examples_footer.html >> website/examples.html
	@echo "✅ Generated website/examples.html successfully"


.PHONY: fmt
fmt:
	@echo "🎨 Formatting Lua code with StyLua"
	@stylua . luacad.lua bin/luacad


.PHONY: release
release: fmt test website/examples.html


.PHONY: clean
clean:
	rm -rf tests/temp
	rm -f examples/*.scad
