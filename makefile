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


# Regenerate the rasterized and path-traced image next to every example's
# entry point (literal_openscad has none: it only emits OpenSCAD code)
.PHONY: example-images
example-images:
	cargo build --package luacad --release
	@for lua in examples/*/*.lua; do \
		case $$lua in examples/literal_openscad/*) continue;; esac; \
		base=$${lua%.lua}; \
		echo "→ $$base.png"; \
		target/release/luacad render "$$lua" "$$base.png"; \
		echo "→ $${base}_raytraced.png"; \
		target/release/luacad render --raytrace "$$lua" \
			"$${base}_raytraced.png"; \
	done


# The browser build behind https://luacad.ad-si.com/playground.
#
# Needs Emscripten on the shell: `nix develop` provides it, as does sourcing
# an emsdk's `emsdk_env.sh`. The per-target compiler variables are set because
# a `CC` inherited from the environment — a Nix dev shell sets one, for
# instance — otherwise wins over the `emcc` the `cc` crate would pick for this
# target on its own.
.PHONY: wasm
wasm:
	@command -v emcc > /dev/null \
		|| (echo "No emcc on this shell: run \`nix develop\`," \
			"or source <emsdk>/emsdk_env.sh" && exit 1)
	CC_wasm32_unknown_emscripten=emcc \
	CXX_wasm32_unknown_emscripten=em++ \
	AR_wasm32_unknown_emscripten=emar \
		cargo build --package luacad-wasm --release \
			--target wasm32-unknown-emscripten
	cp target/wasm32-unknown-emscripten/release/luacad-wasm.js \
		target/wasm32-unknown-emscripten/release/luacad_wasm.wasm \
		website/playground/


# An activated emsdk puts its own root on `PATH`, and that root holds a
# `node` *directory* — which shadows the real interpreter and fails with
# "Permission denied". Emscripten names the node it installed in
# `EMSDK_NODE`, so prefer that whenever it is set.
NODE ?= $(if $(EMSDK_NODE),$(EMSDK_NODE),node)

.PHONY: test-wasm
test-wasm: wasm
	$(NODE) crates/luacad-wasm/smoke_test.mjs website/playground


# Serves the website exactly as GitHub Pages does, so the playground can be
# tried out locally. The wasm module needs a real HTTP server; opening the
# file directly does not work.
.PHONY: serve-website
serve-website: wasm
	@echo "→ http://localhost:8000/playground/"
	cd website && python3 -m http.server 8000


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
	@echo '     cargo publish -p luacad-manifold-sys'
	@echo '     cargo publish -p opencsg-sys'
	@echo '     cargo publish -p luacad'
	@echo '     cargo publish -p luacad-studio'
	@echo '5. Push a `v*` tag, or create the release at' \
		'https://github.com/ad-si/LuaCAD/releases/new'
	@echo '   The `release` job attaches the binaries of every platform and'
	@echo '   their checksums; a bare tag gets a draft release to publish.'
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
