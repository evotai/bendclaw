<p align="center">
  <img src="https://github.com/user-attachments/assets/132aa3cc-5c79-445a-8c18-5da152f7745d" alt="BendClaw" />
</p>

<p align="center">
  <strong>BendClaw</strong>
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
  <a href="#-why-bendclaw">Why</a> &middot;
  <a href="#installation">Install</a> &middot;
  <a href="#quickstart">Quickstart</a> &middot;
  <a href="#development">Dev</a> &middot;
  <a href="#community">Community</a>
</p>

## 📢 News

- **2026-04-11** 🧠 Auto-load Claude Code memories for the current project.
- **2026-04-10** 🎯 Plan mode adds interactive option picker — select choices or provide custom input inline during planning.
- **2026-04-10** 🌐 `web_fetch` supports SPA pages — headless Chrome fallback when static extraction fails.

---

## ⚡ Why BendClaw

Claude Code and Codex dump everything into context — bloated outputs, stale history, noise. Tokens wasted. Quality drops. No visibility into why.

BendClaw doesn't waste a single token — and proves it.

- **Clean context, always.** Every prompt to the LLM is minimal, high-signal, zero-waste.
- **Blazing fast.** Fewer wasted tokens → fewer turns → complex tasks done in half the time.
- **Fully observable.** Every LLM call, tool execution, and compaction tracked end-to-end. This data feeds back into the engine — BendClaw evolves its strategy so the next prompt is always leaner than the last.

Not a CLI wrapper. The agent engine you build on — ships with REPL, CLI, and server.

<p align="center">
  <img width="815" height="768" alt="BendClaw in action" src="https://github.com/user-attachments/assets/f0f14c8c-37f2-4aff-a91a-c30768488b3d" />
</p>

## Installation

### One-liner (recommended)

```bash
curl -fsSL https://raw.githubusercontent.com/evotai/bendclaw/main/install.sh | bash
```

### Build from source

```bash
cargo install --git https://github.com/evotai/bendclaw.git
```

## Quickstart

Create `~/.evotai/bendclaw.env`:

```env
# Provider: "anthropic" or "openai"
BENDCLAW_LLM_PROVIDER=anthropic

# Anthropic
BENDCLAW_ANTHROPIC_API_KEY=sk-ant-...
BENDCLAW_ANTHROPIC_MODEL=claude-sonnet-4-20250514

# OpenAI
BENDCLAW_OPENAI_API_KEY=sk-...
BENDCLAW_OPENAI_MODEL=gpt-4o
```

Only the active provider's keys are required. Set `BENDCLAW_LLM_PROVIDER` to switch.

```bash
bendclaw                              # interactive REPL
bendclaw -p "summarize today's PRs"   # one-shot task
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
make setup      # install Rust toolchain, git hooks
make check      # fmt + clippy
make test       # unit + integration tests
```

## Community

<table>
  <tr>
    <td align="center" width="33%">
      <a href="https://github.com/EvotAI/bendclaw/issues"><b>GitHub Issues</b></a><br>
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
  Built with 🦀 by <a href="https://evot.ai">Evot AI</a>
</p>
