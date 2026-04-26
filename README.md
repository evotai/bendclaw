<p align="center">
  <strong>Evot</strong>
</p>

<p align="center">
  Building software just got 10× faster and token-efficient.
</p>

<p align="center">
  A self-evolving agent engine — fully observable, built for long-running complex work.
</p>

<p align="center">
  The engine behind <a href="https://evot.ai">evot.ai</a>
</p>

<p align="center">
  <a href="#-news">News</a> &middot;
  <a href="#-why-evot">Why</a> &middot;
  <a href="#installation">Install</a> &middot;
  <a href="#quickstart">Quickstart</a> &middot;
  <a href="#development">Dev</a> &middot;
  <a href="#community">Community</a>
</p>

<p align="center">
  <video src="https://github.com/user-attachments/assets/0c089005-51db-48da-977e-6339b5fb9093"></video>
</p>

## 📢 News

- **2026-04-23** 🔍 [Search] Full-text session search — `/resume <query>` to find any past conversation.
- **2026-04-18** 📜 [REPL] `/history` + `/goto` — time-travel through conversation context.
- **2026-04-17** 📋 [REPL] Ctrl+V image paste support.
- **2026-04-13** 🏷️ Project renamed from BendClaw to Evot.
- **2026-04-13** 🔌 [Skills] Auto-load Claude Code skills.

---

## ⚡ Why Evot

Most agents dump everything into context — bloated outputs, stale history, invisible decisions. Tokens burn. Quality drifts.

Evot does the opposite:

- **Zero-waste context.** Every prompt is minimal, high-signal, rebuilt from scratch each turn.
- **Half the tokens, half the time.** Less noise → fewer turns → complex tasks done faster.
- **Self-evolving.** Full observability into every LLM call and tool execution feeds back into the engine — each prompt gets leaner automatically.
- **Everything searchable.** Full-text index over all sessions — `/resume <query>` to find any past conversation, decision, or code snippet instantly.

## Installation

### One-liner (recommended)

```bash
curl -fsSL https://evot.ai/install | sh
```

### From source

```bash
git clone https://github.com/evotai/evot.git
cd evot
make setup && make install
evot
```

## Quickstart

**1. Set your API key**

Create `~/.evotai/evot.env`:

```env
# Anthropic (default)
EVOT_LLM_ANTHROPIC_API_KEY=sk-ant-...
EVOT_LLM_ANTHROPIC_BASE_URL=your-anthropic-base-url
EVOT_LLM_ANTHROPIC_MODEL=claude-opus-4-6

# Or OpenAI
# EVOT_LLM_OPENAI_API_KEY=sk-...
# EVOT_LLM_OPENAI_BASE_URL=your-openai-base-url/v1
# EVOT_LLM_OPENAI_MODEL=gpt-5.5

# Or DeepSeek
# EVOT_LLM_DEEPSEEK_API_KEY=sk-...
# EVOT_LLM_DEEPSEEK_BASE_URL=https://api.deepseek.com
# EVOT_LLM_DEEPSEEK_MODEL=deepseek-v4-pro

# Multiple models under one provider (first is the default)
# EVOT_LLM_ANTHROPIC_MODEL=claude-sonnet-4-6,claude-opus-4-6
```

> Use `--model provider:model` for one-off overrides.

**2. Run**

```bash
evot                                          # interactive REPL
evot -p "summarize today's PRs"               # one-shot task
evot -p "review this" -f ./src/main.rs        # attach file context
evot -p "continue work" -r my-session         # resume or create session
```

<details>
<summary><b>CLI flags & options</b></summary>

| Flag | Description |
|------|-------------|
| `-p, --prompt` | Run a single prompt and exit |
| `-f, --file <path>` | Attach file/directory context |
| `-r, --resume <id>` | Resume or create a session |
| `--model <model>` | Override the configured model |
| `--verbose` | Enable info-level logging |

</details>

## Development

```bash
make setup        # install Rust toolchain, git hooks
make test         # all tests (engine + CLI)
make install      # compile standalone binary to ~/.evotai/bin/evot
```

## Community

## Community

- [**GitHub Issues**](https://github.com/evotai/evot/issues) — Bug reports / Feature
- [**Twitter @Evot_AI**](https://twitter.com/Evot_AI) — Announcements
- [**team@evot.ai**](mailto:team@evot.ai) — Reach the team directly

## License

Apache-2.0

---

<p align="center">
  Built with 🦀 + TypeScript by <a href="https://evot.ai">Evot AI</a>
</p>
