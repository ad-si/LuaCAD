.PHONY: help
help: makefile
	@tail -n +4 makefile | grep ".PHONY"


.PHONY: format
format:
	cargo clippy --fix --allow-dirty
	cargo fmt
	# nix fmt  # TODO: Reactivate when it's faster


.PHONY: lint-lua
lint-lua:
	cargo run --package luacad -- lint examples/


.PHONY: test-units
test-units:
	cargo test --lib --bins -- --show-output
	@echo "✅ All unit tests passed!\n\n"


.PHONY: build
build:
	cargo build


.PHONY: test
test:
	cargo test


.PHONY: run
run:
	cargo run --package luacad-studio


.PHONY: dev
dev:
	watchexec --restart --exts rs,toml -- cargo run --package luacad-studio


.PHONY: release
release:
	@echo '1. `cai changelog <first-commit-hash>`'
	@echo '2. `git add ./changelog.md && git commit -m "Update changelog"`'
	@echo '3. `cargo release major / minor / patch`'
	@echo '4. Create a new GitHub release at' \
		'https://github.com/ad-si/LuaCAD-Studio-Rust/releases/new'
	@echo -e \
		"5. Announce release on \n" \
		"   - https://x.com \n" \
		"   - https://bsky.app \n" \
		"   - https://this-week-in-rust.org \n" \
		"   - https://news.ycombinator.com \n" \
		"   - https://lobste.rs \n" \
		"   - Reddit \n" \
		"     - https://reddit.com/r/rust \n"


.PHONY: install
install:
	cargo install --path crates/luacad
	cargo install --path crates/luacad-studio
