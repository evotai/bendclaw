<p align="center">
  <img src="https://github.com/user-attachments/assets/132aa3cc-5c79-445a-8c18-5da152f7745d" alt="BendClaw" />
</p>

<p align="center">
  <strong>BendClaw</strong> — Blazing-fast autonomous coding agent
</p>

<p align="center">
  Pure Rust. Sub-second startup. Fraction of the memory.<br/>
  Outpaces Claude Code and Codex on long-running, high-complexity work.
</p>

<p align="center">
  The engine behind <a href="https://evot.ai">evot.ai</a>
</p>

---

## Why BendClaw

Claude Code and Codex are powerful — but they're heavy. Node.js runtimes, slow cold starts, high memory baselines, and context windows that bloat as tasks get longer.

BendClaw is built from scratch in Rust to be the fastest coding agent you can run locally:

- **Instant startup** — native binary, no runtime, no interpreter. Ready before your terminal prompt redraws.
- **Tiny footprint** — a fraction of the memory of Node.js-based agents. Runs comfortably on a laptop alongside your IDE, browser, and build tools.
- **Long tasks without choking** — adaptive context management keeps signal high and noise low, even across hundreds of tool calls. Where other agents lose coherence, BendClaw stays on track.
- **Process-group-aware shell execution** — bash commands run in isolated process groups with streaming output, automatic cleanup on timeout/cancel, and 10-minute default timeouts for real-world builds. No orphan processes, no silent hangs.
- **Multi-provider, zero lock-in** — Anthropic, OpenAI, or any OpenAI-compatible endpoint. Switch with one env var.

## How It Compares

| | BendClaw | Claude Code | Codex |
|---|---|---|---|
| Language | Rust | TypeScript/Node | Rust + TypeScript |
| Cold start | ~50ms | ~2s | ~1s |
| Memory baseline | ~15MB | ~200MB+ | ~100MB+ |
| Long-task context | Adaptive compaction | Fixed window | Fixed window |
| Shell cleanup | Process group kill | Process group kill | Process group kill |
| Streaming shell output | ✓ | ✓ | ✓ |
| Install | Single binary | npm | npm + Rust |

## What BendClaw Does

BendClaw is an autonomous agent for long-running, high-complexity work.

It keeps tasks moving by maintaining useful working context:
preserving what still matters, compressing what can be distilled, and shedding what has turned into noise.

As runs get longer, BendClaw works to keep focus, continuity, and task signal intact instead of letting tool output and stale history take over.

## Why It Gets Better Over Time

BendClaw improves through an observable, auditable feedback loop: it watches execution, refines context, and evolves toward the task.

Different tasks need different working memory.
Some tasks need precise state.
Some need compressed findings.
Some need the latest decisions, failures, and changes.

Over time, BendClaw gets better at keeping what helps, compressing what matters, and leaving behind what does not.

## Example

<img width="815" height="768" alt="Image" src="https://github.com/user-attachments/assets/f0f14c8c-37f2-4aff-a91a-c30768488b3d" />

```text
BendClaw is not trying to keep everything.

It tries to turn this:

  long prompt
  + repeated history
  + oversized tool results
  + stale intermediate state

into this:

  larger useful context
  + preserved task signal
  + compressed supporting history
  + only the tool output that still matters
```

## Installation

```bash
# One-liner install (prebuilt binary, recommended)
curl -fsSL https://raw.githubusercontent.com/evotai/bendclaw/main/install.sh | bash

# Or build from source
cargo install --git https://github.com/evotai/bendclaw.git
```

## Quickstart

Create `~/.evotai/bendclaw.env`:

Example:

```env
# Provider: "anthropic" or "openai"
BENDCLAW_LLM_PROVIDER=anthropic

# Anthropic
BENDCLAW_ANTHROPIC_API_KEY=sk-ant-...
BENDCLAW_ANTHROPIC_MODEL=claude-opus-4-6
BENDCLAW_ANTHROPIC_BASE_URL=https://...

# OpenAI
BENDCLAW_OPENAI_API_KEY=sk-...
BENDCLAW_OPENAI_MODEL=gpt-5.4
BENDCLAW_OPENAI_BASE_URL=https://...
```

Only the active provider's keys are required. Set `BENDCLAW_LLM_PROVIDER` to switch between them.

Then run:

```bash
bendclaw
```

Or a one-shot task:

```bash
bendclaw -p "summarize today's PRs"
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
