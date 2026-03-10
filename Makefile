# BendClaw

DEV_CONFIG ?= $(HOME)/.bendclaw/bendclaw_dev.toml

.PHONY: setup check run test test-unit test-integration test-contract test-e2e test-all coverage snapshot-review dev-env test-down ci

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
		echo "==> preparing boxlite runtime..."; \
		BOXLITE_DEPS_STUB=2 cargo test --test unit --no-run 2>/dev/null; \
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

# Fast: unit + integration + contract (no credentials needed, < 30s)
test: test-unit test-integration test-contract

test-unit:
	cargo nextest run --test unit --no-fail-fast

test-integration:
	cargo nextest run --test integration --no-fail-fast

test-contract:
	cargo nextest run --test contract --no-fail-fast

# Requires Databend credentials
test-e2e: dev-env
	RUST_LOG=ERROR cargo nextest run --test e2e --no-fail-fast

# Everything
test-all: test test-e2e

coverage:
	cargo install cargo-llvm-cov --locked 2>/dev/null || true
	cargo llvm-cov nextest --test unit --test integration --test contract \
		--ignore-run-fail --html --output-dir coverage
	@echo "==> coverage report: coverage/html/index.html"

coverage-all: dev-env
	cargo install cargo-llvm-cov --locked 2>/dev/null || true
	cargo llvm-cov nextest --test unit --test integration --test contract --test e2e \
		--ignore-run-fail --html --output-dir coverage
	@echo "==> coverage report: coverage/html/index.html"

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
