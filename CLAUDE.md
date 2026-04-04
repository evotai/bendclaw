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
- `open-agent-sdk` lives in `crates/open-agent-sdk` as a git submodule — treat it as a dependency, do not modify it directly

# Testing

- Feel free to refactor tests aggressively; write tests based on the new code
- Avoid glue test code; write clear and explicit tests
- Fast iteration is expected
- All tests must go in the `tests/` directory, never inline in source files
- Focus on core logic coverage, not overall coverage
