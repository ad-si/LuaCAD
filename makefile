.PHONY: help
help: makefile
	@tail -n +4 makefile | grep ".PHONY"


.PHONY: format
format:
	cargo clippy --fix --allow-dirty
	cargo fmt
	# nix fmt  # TODO: Reactivate when it's faster


.PHONY: lint
lint:
	cargo fmt --all --check
	cargo clippy --workspace --all-targets -- --deny warnings


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


.PHONY: package
package:
	cargo package --workspace


.PHONY: release
release:
	@echo '1. `cai changelog <first-commit-hash>`'
	@echo '2. `git add ./changelog.md && git commit -m "Update changelog"`'
	@echo '3. Check CI is green on main — the `package` job verifies every'
	@echo '   crate builds from its tarball. `make package` does the same'
	@echo '   locally, but reuses cached builds of the local registry and can'
	@echo '   pass or fail against stale artifacts; trust CI over it.'
	@echo '4. Publish in dependency order — each must be live on crates.io'
	@echo '   before the next one can resolve it:'
	@echo '     cargo publish -p manifold-sys'
	@echo '     cargo publish -p opencsg-sys'
	@echo '     cargo publish -p luacad'
	@echo '     cargo publish -p luacad-studio'
	@echo '5. Create a new GitHub release at' \
		'https://github.com/ad-si/LuaCAD/releases/new'
	@echo -e \
		"6. Announce release on \n" \
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
