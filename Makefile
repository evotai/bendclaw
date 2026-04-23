# Evot

DEV_CONFIG ?= $(HOME)/.evotai/evot.env
CARGO ?= cargo
BUN_DIR := $(HOME)/.bun/bin
export PATH := $(BUN_DIR):$(PATH)
NEXTEST := $(CARGO) nextest run --no-tests=pass
COVERAGE_TARGETS := --workspace --exclude evot-napi
COVERAGE_CMD := $(CARGO) llvm-cov nextest $(COVERAGE_TARGETS)

.PHONY: setup check build run test test-engine test-cli test-tui test-rust coverage coverage-report snapshot-review dev-env ci build-napi build-cli install

setup:
	@set -e; \
	export PATH="$$HOME/.bun/bin:$$HOME/.cargo/bin:$$PATH"; \
	need_cmd() { command -v "$$1" >/dev/null 2>&1; }; \
	need_fetch() { \
		if need_cmd curl; then \
			curl -fsSL "$$1"; \
		elif need_cmd wget; then \
			wget -qO- "$$1"; \
		else \
			echo "    error: neither curl nor wget is available" >&2; \
			exit 1; \
		fi; \
	}; \
	ensure_rustup() { \
		if need_cmd cargo; then \
			return 0; \
		fi; \
		echo "==> installing rustup..."; \
		need_fetch https://sh.rustup.rs | sh -s -- -y --profile minimal --default-toolchain none; \
		export PATH="$$HOME/.cargo/bin:$$PATH"; \
		need_cmd cargo || { echo "    error: rustup installation failed" >&2; exit 1; }; \
	}; \
	ensure_bun() { \
		if need_cmd bun; then \
			return 0; \
		fi; \
		echo "==> installing bun..."; \
		need_fetch https://bun.sh/install | bash; \
		export PATH="$$HOME/.bun/bin:$$PATH"; \
		need_cmd bun || { echo "    error: bun installation failed" >&2; exit 1; }; \
	}; \
	ensure_rustup; \
	ensure_bun; \
	echo "==> initializing submodules..."; \
	git submodule update --init --recursive; \
	if [ "$$(uname -s)" = "Darwin" ]; then \
		echo "==> pre-compiling rust tests..."; \
		cargo test --workspace --exclude evot-napi --no-run 2>/dev/null || true; \
	fi
	@echo "==> installing git hooks..."
	@mkdir -p .git/hooks
	@printf '#!/bin/sh\nexport PATH="$$HOME/.cargo/bin:$$PATH"\ncargo fmt --all\ngit diff --name-only --diff-filter=M | xargs git add\n' > .git/hooks/pre-commit
	@chmod +x .git/hooks/pre-commit
	@printf '#!/bin/sh\nexport PATH="$$HOME/.cargo/bin:$$PATH"\nmake check\n' > .git/hooks/pre-push
	@chmod +x .git/hooks/pre-push
	@echo "==> setup complete"

check:
	cargo fmt --all -- --check
	cargo clippy --all-targets -- -D warnings

build: build-napi build-cli

test: test-engine test-cli

test-engine: test-rust

test-rust:
	$(NEXTEST) --workspace --exclude evot-napi --no-fail-fast

test-cli:
	cd cli && bun test tests/ --path-ignore-patterns 'tests/tui/**'

test-tui: build-cli
	cd cli && bunx @microsoft/tui-test tests/tui

coverage: coverage-core-check

coverage-report:
	$(CARGO) install cargo-llvm-cov --locked 2>/dev/null || true
	$(COVERAGE_CMD) --ignore-run-fail --codecov --output-path codecov.json
	$(CARGO) llvm-cov report --html --output-dir coverage
	$(CARGO) llvm-cov report > coverage-summary.txt

coverage-core-check: coverage-report
	python3 scripts/check_core_coverage.py coverage-summary.txt

snapshot-review:
	cargo insta review

dev-env:
	@if [ ! -f $(DEV_CONFIG) ]; then \
		echo "==> creating dev config at $(DEV_CONFIG)"; \
		mkdir -p $$(dirname $(DEV_CONFIG)); \
		printf 'EVOT_LLM_PROVIDER=anthropic\nEVOT_LLM_ANTHROPIC_API_KEY=\nEVOT_LLM_ANTHROPIC_BASE_URL=https://api.anthropic.com\nEVOT_LLM_ANTHROPIC_MODEL=claude-opus-4-6\n' > $(DEV_CONFIG); \
	fi
	@echo ""
	@echo "  DEV MODE"
	@echo "  config: $(DEV_CONFIG)"
	@echo ""

run: dev-env build
	./cli/bin/evot

ci: check test

# -- TS CLI -------------------------------------------------------------------

build-napi:
	cd cli && bun install && bunx napi build --manifest-path addon/Cargo.toml --release --platform --output-dir .

build-cli: build-napi
	cd cli && bun build src/index.ts --compile --define 'process.env.DEV="false"' --define 'process.env.NODE_ENV="production"' --outfile dist/evot

install: build-cli
	@mkdir -p $(HOME)/.evotai/bin $(HOME)/.evotai/lib
	rm -f $(HOME)/.evotai/bin/evot
	cp cli/dist/evot $(HOME)/.evotai/bin/evot
	cp cli/evot-napi.*.node $(HOME)/.evotai/lib/
	@if [ "$$(uname -s)" = "Darwin" ]; then \
		xattr -cr $(HOME)/.evotai/bin/evot; \
		codesign --force --sign - $(HOME)/.evotai/bin/evot; \
		for f in $(HOME)/.evotai/lib/evot-napi.*.node; do \
			xattr -cr "$$f"; \
			codesign --force --sign - "$$f"; \
		done; \
	fi
	@INSTALL_DIR="$(HOME)/.evotai/bin"; \
	ENV_FILE="$(HOME)/.evotai/evot.env"; \
	echo ""; \
	echo "  ✓ Installed evot to $$INSTALL_DIR/evot"; \
	echo "  ✓ Copied .node bindings to $(HOME)/.evotai/lib/"; \
	echo ""; \
	if [ ! -f "$$ENV_FILE" ]; then \
		printf 'EVOT_LLM_PROVIDER=anthropic\nEVOT_LLM_ANTHROPIC_API_KEY=\nEVOT_LLM_ANTHROPIC_BASE_URL=https://api.anthropic.com\nEVOT_LLM_ANTHROPIC_MODEL=claude-opus-4-6\n' > "$$ENV_FILE"; \
		echo "  ✓ Created config at $$ENV_FILE"; \
		echo "    Edit it to set your API keys and provider settings."; \
		echo ""; \
	fi; \
	if ! echo "$$PATH" | tr ':' '\n' | grep -qx "$$INSTALL_DIR"; then \
		SHELL_NAME="$$(basename "$${SHELL:-/bin/bash}")"; \
		case "$$SHELL_NAME" in \
			zsh)  RC="$(HOME)/.zshrc" ;; \
			bash) RC="$(HOME)/.bashrc" ;; \
			fish) RC="$(HOME)/.config/fish/config.fish" ;; \
			*)    RC="$(HOME)/.profile" ;; \
		esac; \
		if [ "$$SHELL_NAME" = "fish" ]; then \
			grep -qF "$$INSTALL_DIR" "$$RC" 2>/dev/null || \
				echo "set -Ux fish_user_paths $$INSTALL_DIR \$$fish_user_paths" >> "$$RC"; \
		else \
			grep -qF "$$INSTALL_DIR" "$$RC" 2>/dev/null || \
				echo "export PATH=\"$$INSTALL_DIR:\$$PATH\"" >> "$$RC"; \
		fi; \
		echo "  ✓ Added $$INSTALL_DIR to PATH in $$RC"; \
		echo "    Run: source $$RC"; \
		echo ""; \
	fi

build-napi-dev:
	cd cli && bun install && bunx napi build --manifest-path addon/Cargo.toml --platform --output-dir .

dev: build-napi-dev
	cd cli && bun install && bun run src/index.ts

check-cli:
	cd cli && bun run src/index.ts --version >/dev/null
