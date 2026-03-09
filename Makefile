# BendClaw

DEV_CONFIG ?= $(HOME)/.bendclaw/bendclaw_dev.toml

.PHONY: setup check run test dev-env test-down ci coverage

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
		BOXLITE_DEPS_STUB=2 cargo test --test it --no-run 2>/dev/null; \
	fi
	@echo "==> installing pre-push hook..."
	@mkdir -p .git/hooks
	@echo '#!/bin/sh\nmake check' > .git/hooks/pre-push
	@chmod +x .git/hooks/pre-push
	@echo "==> setup complete"

check:
	cargo fmt --all -- --check
	cargo clippy --all-targets -- -D warnings

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

test: dev-env
	RUST_LOG=ERROR cargo nextest run --test it --no-fail-fast

coverage: dev-env
	cargo install cargo-llvm-cov --locked 2>/dev/null || true
	cargo llvm-cov nextest --test it -E 'not (test(/^service::api::/) or test(/^service::admin::/) or test(/^service::e2e::/) or test(/^service::handler::/))' --ignore-run-fail --html --output-dir coverage
	@echo "==> coverage report: coverage/html/index.html"

ci: check test

test-down:
	@echo "==> nothing to stop (no local databend)"
