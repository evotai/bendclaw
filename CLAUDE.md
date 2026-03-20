# Coding Standards

- Never use `unwrap()` or `expect()`
- Always propagate errors with `?` or handle them explicitly using `match` / `if let`

# Architecture

- Keep design simple, clear, and decoupled
- Promote code reuse
- Use OOP principles without over-engineering
- Design for testability

# Testing

- Feel free to refactor tests aggressively; write tests based on the new code
- Avoid glue test code; write clear and explicit tests
- Fast iteration is expected
- All tests must go in the `tests/` directory, never inline in source files
- Focus on core logic coverage, not overall coverage
