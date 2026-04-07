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


**Evolve** — Post-run recall extracts learnings from execution. Shared memory makes knowledge available to all agents on the team. The replay → memory loop means agents don't just accumulate knowledge blindly — they learn from what actually happened.

## Usage

```bash
# Interactive REPL (default)
bendclaw

# One-shot prompt
bendclaw -p "summarize today's PRs"

# Resume a previous session
bendclaw --resume <session_id>

# HTTP / SSE server
bendclaw server --port 8080
```

| Flag | Description |
|---|---|
| `-p, --prompt` | Run a single prompt and exit |
| `--resume <id>` | Resume an existing session |
| `--model <model>` | Override the configured model |
| `--output-format text\|stream-json` | Output format (default: text) |
| `--max-turns <n>` | Limit agent turns (default: 512) |
| `--max-tokens <n>` | Limit total tokens |
| `--max-duration <secs>` | Session timeout in seconds (default: 3600) |
| `--append-system-prompt "..."` | Inject extra system instructions |
| `--verbose` | Enable info-level logging |

## Configuration

Create `~/.evotai/bendclaw.env` with your LLM provider settings:

```env
# Provider: "anthropic" or "openai"
BENDCLAW_LLM_PROVIDER=anthropic

# Anthropic
BENDCLAW_ANTHROPIC_API_KEY=sk-ant-...
BENDCLAW_ANTHROPIC_MODEL=claude-opus-4-6
# Optional: custom base URL (omit to use the default Anthropic endpoint)
# BENDCLAW_ANTHROPIC_BASE_URL=https://...

# OpenAI
BENDCLAW_OPENAI_API_KEY=sk-...
BENDCLAW_OPENAI_MODEL=gpt-5.4
# Optional: custom base URL (omit to use the default OpenAI endpoint)
# BENDCLAW_OPENAI_BASE_URL=https://...
```

Only the active provider's keys are required. Set `BENDCLAW_LLM_PROVIDER` to switch between them.

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
