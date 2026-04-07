<p align="center">
  <img src="https://github.com/user-attachments/assets/132aa3cc-5c79-445a-8c18-5da152f7745d" alt="BendClaw" />
</p>

<p align="center">
  <strong>BendClaw</strong>
</p>

<p align="center">
  Self-evolving AI agents. Share everything. Get better every run.
</p>

<p align="center">
  A Rust-native runtime where agents learn from execution, inspect their own behavior, and co-evolve — no prompt engineering required.
</p>

<p align="center">
  The engine behind <a href="https://evot.ai">evot.ai</a>
</p>

---

## How Self-Evolution Works

```
  ┌─────────┐     ┌─────────┐     ┌─────────┐
  │ Execute │────▶│ Observe │────▶│  Evolve │
  └─────────┘     └─────────┘     └─────────┘
       ▲                                │
       └────────────────────────────────┘
```

**Execute** — The kernel runs agent sessions: LLM reasoning, tool calls, skill execution, context compaction. Every tool invocation and capability snapshot is recorded as a structured event.

**Observe** — Session replay projects raw events into an inspectable summary: which tools were called, what succeeded or failed, what capabilities were available, how the session ended. Agents can call this API on their own sessions.

```
GET /v1/agents/{agent_id}/workbench/sessions/{session_id}/replay
```

**Evolve** — Post-run recall extracts learnings from execution. Shared memory makes knowledge available to all agents on the team. The replay → memory loop means agents don't just accumulate knowledge blindly — they learn from what actually happened.

## Development

```bash
make setup      # install Rust toolchain, git hooks
make check      # fmt + clippy
make test       # unit + integration + contract
```

## Community

- [GitHub Issues](https://github.com/EvotAI/bendclaw/issues) — bug reports & feature requests
- [Twitter @Evot_AI](https://twitter.com/Evot_AI) — updates & announcements
- team@evot.ai — reach the team directly

## License

Apache-2.0
