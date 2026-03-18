# BendClaw

DEV_CONFIG ?= $(HOME)/.bendclaw/bendclaw_dev.toml
CARGO ?= cargo
NEXTEST := $(CARGO) nextest run
LIVE_TEST_FLAGS := --features live-tests -- --test-threads=1
COVERAGE_TARGETS := --lib --test unit --test it --test contract
COVERAGE_CMD := $(CARGO) llvm-cov nextest $(COVERAGE_TARGETS)

.PHONY: setup check build run test test-fast test-unit test-it test-contract test-live test-live-storage test-live-e2e test-all coverage coverage-report coverage-core-check snapshot-review dev-env test-down ci

setup:
	@echo "==> checking protoc..."
	@if command -v protoc >/dev/null 2>&1; then \
		echo "    protoc found: $$(protoc --version)"; \
	else \
		echo "    installing protoc..."; \
		if [ "$$(uname -s)" = "Darwin" ]; then brew install protobuf; \
		else sudo apt-get update -qq && sudo apt-get install -y -qq protobuf-compiler; fi; \
	fi
	@if [ "$$(uname -s)" = "Darwin" ]; then \
		echo "==> pre-compiling unit tests..."; \
		cargo test --test unit --no-run 2>/dev/null; \
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

# Fast: unit + it + contract (no credentials needed)
test: test-fast

test-fast: test-unit test-it test-contract

test-unit:
	$(NEXTEST) --lib --test unit --no-fail-fast

test-it:
	$(NEXTEST) --test it --no-fail-fast

test-contract:
	$(NEXTEST) --test contract --no-fail-fast

# Requires Databend credentials.
# Minimal live suite:
# 1. Databend-backed storage contracts
# 2. API end-to-end smoke flows
test-live: test-live-storage test-live-e2e

test-live-storage: dev-env
	RUST_LOG=ERROR $(CARGO) test --test live-storage-contract $(LIVE_TEST_FLAGS)

test-live-e2e: dev-env
	RUST_LOG=ERROR $(CARGO) test --test live-api-e2e $(LIVE_TEST_FLAGS)

# Everything
test-all: test-fast test-live

coverage: coverage-core-check

coverage-report:
	$(CARGO) install cargo-llvm-cov --locked 2>/dev/null || true
	$(COVERAGE_CMD) --ignore-run-fail --codecov --output-path codecov.json
	$(CARGO) llvm-cov report --html --output-dir coverage
	$(CARGO) llvm-cov report > coverage-summary.txt

coverage-core-check: coverage-report
	python3 scripts/check_core_coverage.py coverage-summary.txt

coverage-all: coverage-core-check

snapshot-review:
	cargo insta review

# Ensure dev config exists.
dev-env:
	@if [ ! -f $(DEV_CONFIG) ]; then \
		echo "==> creating dev config at $(DEV_CONFIG)"; \
		mkdir -p $$(dirname $(DEV_CONFIG)); \
		cp configs/bendclaw.toml.example $(DEV_CONFIG); \
	fi
	@echo ""
	@echo "  DEV MODE"
	@echo "  api:    localhost:8787"
	@echo "  config: $(DEV_CONFIG)"
	@echo "  databend: https://app.databend.com"
	@echo ""

run: dev-env
	cargo run -- --config $(DEV_CONFIG) run

ci: check test

test-down:
	@echo "==> nothing to stop (no local databend)"
