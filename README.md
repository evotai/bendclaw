<p align="center">
  <img src="https://pbs.twimg.com/profile_banners/2024395183564214273/1773027467/1500x500" alt="Evot" />
</p>

<p align="center">
  <strong>Evot</strong>
</p>

<p align="center">
  A self-evolving agent engine — fully observable, token-efficient, built for long-running complex work.
</p>

<p align="center">
  Everything Claude Code / Codex can do — half the tokens, fully observable, in 20k+ lines of Rust.
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

## 📢 News

- **2026-04-18** 📜 [REPL] `/history` + `/goto` — time-travel through conversation context.
- **2026-04-17** 📋 [REPL] Ctrl+V image paste support.
- **2026-04-13** 🏷️ Project renamed from BendClaw to Evot.
- **2026-04-13** 🔌 [Skills] Auto-load Claude Code skills.
- **2026-04-11** 🧠 [Memory] Auto-load Claude Code memories for the current project.
- **2026-04-10** 🎯 [Plan Mode] Add `ask_user` tool for interactive option selection.
- **2026-04-10** 🌐 [Web Fetch] SPA support via headless Chrome fallback.

---

## ⚡ Why Evot

Claude Code and Codex dump everything into context — bloated outputs, stale history, noise. Tokens wasted. Quality drops. No visibility into why.

Evot doesn't waste a single token — and proves it.

- **Clean context, always.** Every prompt to the LLM is minimal, high-signal, zero-waste.
- **Blazing fast.** Fewer wasted tokens → fewer turns → complex tasks done in half the time.
- **Fully observable.** Every LLM call, tool execution, and compaction tracked end-to-end. This data feeds back into the engine — Evot evolves its strategy so the next prompt is always leaner than the last.

Not a CLI wrapper. The agent engine you build on — ships with interactive REPL, CLI, and server.

Built on a Rust engine with a TypeScript CLI powered by Ink.

<p align="center">
  <img width="815" height="768" alt="Evot in action" src="https://github.com/user-attachments/assets/f0f14c8c-37f2-4aff-a91a-c30768488b3d" />
</p>

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

Create `~/.evotai/evot.env`:

```env
# Active provider: anthropic, openai, deepseek and others
EVOT_LLM_PROVIDER=anthropic

# Anthropic
EVOT_LLM_ANTHROPIC_API_KEY=sk-ant-...
EVOT_LLM_ANTHROPIC_BASE_URL=https://api.anthropic.com or custom URL
EVOT_LLM_ANTHROPIC_MODEL=claude-sonnet-4-6

# OpenAI
# EVOT_LLM_OPENAI_API_KEY=sk-...
# EVOT_LLM_OPENAI_BASE_URL=https://api.openai.com/v1 or custom URL
# EVOT_LLM_OPENAI_MODEL=gpt-5.4

# DeepSeek
# EVOT_LLM_DEEPSEEK_API_KEY=sk-ds-...
# EVOT_LLM_DEEPSEEK_BASE_URL=https://api.deepseek.com
# EVOT_LLM_DEEPSEEK_MODEL=deepseek-chat
```

Configure as many providers as you need. Set `EVOT_LLM_PROVIDER` to switch the active one.
Use `--model <name>` to switch by model name, or `--model provider:model` for precise control.

```bash
evot                              # interactive REPL
evot -p "summarize today's PRs"   # one-shot task
```

<details>
<summary><b>CLI flags & options</b></summary>

| Flag | Description |
|------|-------------|
| `-p, --prompt` | Run a single prompt and exit |
| `--resume <id>` | Resume an existing session |
| `--model <model>` | Override the configured model |
| `--output-format text\|stream-json` | Output format (default: text) |
| `--max-turns <n>` | Limit agent turns (default: 512) |
| `--max-tokens <n>` | Limit total tokens |
| `--max-duration <secs>` | Session timeout in seconds (default: 3600) |
| `--append-system-prompt "..."` | Inject extra system instructions |
| `--verbose` | Enable info-level logging |

</details>

## Development

```bash
make setup        # install Rust toolchain, git hooks
make check        # fmt + clippy
make test         # all tests (engine + CLI)
make test-engine  # Rust engine tests only
make test-cli     # CLI tests only (TypeScript)
make dev          # build NAPI + run CLI in dev mode
make install      # compile standalone binary to ~/.evotai/bin/evot
```

## Community

<table>
  <tr>
    <td align="center" width="33%">
      <a href="https://github.com/evotai/evot/issues"><b>GitHub Issues</b></a><br>
      <sub>Bug reports / Feature</sub>
    </td>
    <td align="center" width="33%">
      <a href="https://twitter.com/Evot_AI"><b>Twitter @Evot_AI</b></a><br>
      <sub>Updates & announcements</sub>
    </td>
    <td align="center" width="33%">
      <a href="mailto:team@evot.ai"><b>team@evot.ai</b></a><br>
      <sub>Reach the team directly</sub>
    </td>
  </tr>
</table>

## License

Apache-2.0

---

<p align="center">
  Built with 🦀 + TypeScript by <a href="https://evot.ai">Evot AI</a>
</p>
