# Evot

DEV_CONFIG ?= $(HOME)/.evotai/evot.env
CARGO ?= cargo
NEXTEST := $(CARGO) nextest run --no-tests=pass
COVERAGE_TARGETS := --lib --test unit --test it --test contract
COVERAGE_CMD := $(CARGO) llvm-cov nextest $(COVERAGE_TARGETS)

.PHONY: setup check build run test test-fast test-unit test-it test-contract test-ui coverage coverage-report snapshot-review dev-env ci build-napi build-ui

setup:
	@set -e; \
	export PATH="$$HOME/.cargo/bin:$$PATH"; \
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
	ensure_rustup; \
	echo "==> initializing submodules..."; \
	git submodule update --init --recursive; \
	if [ "$$(uname -s)" = "Darwin" ]; then \
		echo "==> pre-compiling unit tests..."; \
		cargo test --test unit --no-run 2>/dev/null || true; \
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

build:
	cargo build --release

test: test-fast

test-fast: test-unit test-it test-contract

test-unit:
	$(NEXTEST) --lib --test unit --no-fail-fast

test-it:
	$(NEXTEST) --test it --no-fail-fast

test-contract:
	$(NEXTEST) --test contract --no-fail-fast

test-ui:
	cd cli && bun test tests/

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
		cp configs/evot.env.example $(DEV_CONFIG); \
	fi
	@echo ""
	@echo "  DEV MODE"
	@echo "  config: $(DEV_CONFIG)"
	@echo ""

run: dev-env
	cargo run -p evot -- repl

ci: check test

# -- TS CLI (Ink UI) ---------------------------------------------------------

build-napi:
	cd cli && napi build --manifest-path ../src/napi/Cargo.toml --release --platform

build-napi-dev:
	cd cli && napi build --manifest-path ../src/napi/Cargo.toml --platform -o .

build-ui: build-napi
	cd cli && bun install

build-ui-dev: build-napi-dev
	cd cli && bun install

run-ui: build-ui
	cd cli && bun run src/index.tsx

dev-ui: build-ui-dev
	cd cli && bun run src/index.tsx

check-ui:
	cd cli && bun run src/index.tsx --version >/dev/null
