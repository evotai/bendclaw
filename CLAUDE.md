# Coding Standards

- Never use `unwrap()` or `expect()`
- Always propagate errors with `?` or handle them explicitly using `match` / `if let`

# Architecture

- Keep design simple, clear, and decoupled
- Promote code reuse
- Use OOP principles without over-engineering
- Design for testability
- `mod.rs` files must only contain module declarations (`mod`), re-exports (`pub use`), and `use` statements — no code logic
- `lib.rs` files follow the same rule: only module declarations, re-exports, and `use` statements — no business logic
- `bend-agent` lives in `crates/bend-agent` as a workspace member — the core agent runtime engine

# Testing

- Feel free to refactor tests aggressively; write tests based on the new code
- Avoid glue test code; write clear and explicit tests
- Fast iteration is expected
- All tests must go in the `tests/` directory, never inline in source files
- Focus on core logic coverage, not overall coverage

# Pre-commit

- Always run `make check` before committing — it runs `cargo fmt --check` and `cargo clippy`
