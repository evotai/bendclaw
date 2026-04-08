<p align="center">
  <img src="https://github.com/user-attachments/assets/132aa3cc-5c79-445a-8c18-5da152f7745d" alt="BendClaw" />
</p>

<p align="center">
  <strong>BendClaw</strong>
</p>

<p align="center">
  A self-evolving agent built for long-running, high-complexity work.
</p>

<p align="center">
  BendClaw handles longer, harder tasks through a continuous feedback loop: observe execution, refine context, and evolve toward the task.
</p>

<p align="center">
  The engine behind <a href="https://evot.ai">evot.ai</a>
</p>

---

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

```text
[LLM] Complete
  61,001 input · 248 output tokens
  timing   320ms ttfb · 1.8s ttft · 14.7s stream · 16.8s total

[COMPACT] Complete
  saved ~140k tokens (44%)
  320k → 180k

─── This Run Summary ──────────────────────────────────
226.5s · 11 turns · 11 llm calls · 10 tool calls · 750142 tokens

  tokens    750142 total input · 1796 output · 9.9 tok/s
            system          12k  █░░░░░░░░░░░░░░░░░░░  1.7%
            user             8k  █░░░░░░░░░░░░░░░░░░░  1.1%
            assistant       25k  █░░░░░░░░░░░░░░░░░░░  3.3%
            tool_result    702k  ███████████████████░ 93.6%
              read_file   5 calls   312k  ████████░░░░░░░░░░░░ 41.6%
              search      3 calls    98k  ███░░░░░░░░░░░░░░░░░ 13.1%

  compact   1 compactions · saved 140k tokens
            #1  lv1  320k→180k  saved 140k  █████░░░░░░ 44%

  llm       11 calls · 181.2s (80% of run) · 9.9 tok/s avg
            ttft avg 1.8s · stream avg 14.7s
```

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
# cargo binstall (prebuilt binary, recommended)
cargo binstall bendclaw

# Or build from source
cargo install --git https://github.com/EvotAI/bendclaw.git
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
