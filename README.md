<p align="center">
  <strong>Evot</strong>
</p>

<p align="center"><strong>High-quality AI work, at low or even zero cost.</strong></p>

<p align="center">We pick great models — including free stealth ones like <code>stealth/ox-alpha</code> (free this week) — so everyone can run long tasks affordably.</p>

<p align="center">
  <a href=".github/assets/demo.gif"><img src=".github/assets/demo.gif" alt="evot demo" width="960" /></a>
</p>

## 📢 News

- **2026-09-02** `ctrl+b` backgrounds a long-running command so you can keep talking.
- **2026-09-01** `GPT-5.6 Luna` is free through Sep 3 🎉.
- **2026-08-31** [herdr](https://herdr.dev) works with evot 🐑 — point `EVOT_SESSION_HOOK` at an adapter and your panes show `working` / `blocked` / `idle` live.
- **2026-08-27** `GPT-5.6 Luna` is free through Aug 31 🎉 — just `evot login`.
- **2026-08-24** Free model of the week: [`stealth/ox-alpha`](https://openrouter.ai/stealth/ox-alpha) — free on OpenRouter for a week.

## Performance

Same task, same eval environment, three agents × three models — cost and tool calls, lower is better.

<p align="center">
  <a href=".github/assets/benchmark-agent-model-comparison.png"><img src=".github/assets/benchmark-agent-model-comparison.png" alt="Benchmark comparing evot, Claude Code, and pi" width="960" /></a>
</p>

> Task: fix a real bug in serde_json ([issue #979](https://github.com/serde-rs/json/issues/979)) end to end.

All nine runs produce correct, passing code — but evot costs **72–78% less** than Claude Code with **fewer tool calls** on every model.

## Installation

```bash
curl -fsSL https://evot.ai/install | sh
```

<details>
<summary>Build from source</summary>

```bash
git clone https://github.com/evotai/evot.git
cd evot
make setup && make install
```

</details>

## Login

```bash
evot login     # follow the prompts; you land straight in the TUI after login
```

```bash
evot           # interactive TUI
evot -c        # continue the latest session in this directory
```

> In the TUI: `/help` lists all commands.

## External session hooks

Evot can emit generic session lifecycle events to an external executable without embedding integration-specific code:

```bash
EVOT_SESSION_HOOK=/path/to/session-adapter evot
```

The adapter receives one versioned NDJSON object per line on stdin. Events include `session_started`, `run_started`, `run_finished`, `run_failed`, `state_changed`, and `session_ended`; `state_changed.state` is `working`, `blocked`, or `idle`. Session identity is available as `session_id`, and run identity as `run_id`.

The hook is best-effort: a missing or failing adapter never changes evot's main execution path. The same protocol is used by the interactive TUI and one-shot `evot -p` mode, so adapters can remain independent of the UI.

<details>
<summary>Custom configuration (bring your own models via <code>~/.evotai/evot.env</code>)</summary>

```env
# Anthropic (default)
EVOT_LLM_ANTHROPIC_API_KEY=sk-ant-...
EVOT_LLM_ANTHROPIC_BASE_URL=your-anthropic-base-url
EVOT_LLM_ANTHROPIC_MODEL=claude-opus-4.8
# Multiple models: EVOT_LLM_ANTHROPIC_MODEL=claude-sonnet-5.0,claude-opus-4.8,claude-fable-5

# Or OpenAI Chat Completions
# EVOT_LLM_OPENAI_API_KEY=sk-...
# EVOT_LLM_OPENAI_BASE_URL=your-openai-compatible-base-url
# EVOT_LLM_OPENAI_MODEL=gpt-5.6-sol
# EVOT_LLM_OPENAI_PROTOCOL=openai

# Or OpenAI Responses API (official OpenAI GPT/Codex models)
# Using the official endpoint enables provider-native "remote compaction":
# context is compacted server-side with far higher recall, taking priority
# over the local algorithmic path (auto — falls back to local on any failure).
# EVOT_LLM_OPENAI_API_KEY=sk-...
# EVOT_LLM_OPENAI_MODEL=gpt-5.6-sol
# EVOT_LLM_OPENAI_PROTOCOL=openai_responses

# Or DeepSeek (Anthropic-compatible)
# EVOT_LLM_DEEPSEEK_API_KEY=sk-...
# EVOT_LLM_DEEPSEEK_BASE_URL=https://api.deepseek.com/anthropic
# EVOT_LLM_DEEPSEEK_PROTOCOL=anthropic
# EVOT_LLM_DEEPSEEK_MODEL=deepseek-v4-pro

# Or Kimi Coding (Anthropic-compatible)
# EVOT_LLM_KIMI_API_KEY=sk-...
# EVOT_LLM_KIMI_BASE_URL=https://api.kimi.com/coding
# EVOT_LLM_KIMI_PROTOCOL=anthropic
# EVOT_LLM_KIMI_MODEL=kimi-for-coding

# Or OpenRouter (Anthropic-compatible)
# EVOT_LLM_OPENROUTER_API_KEY=sk-or-...
# EVOT_LLM_OPENROUTER_BASE_URL=https://openrouter.ai/api/
# EVOT_LLM_OPENROUTER_PROTOCOL=anthropic
# EVOT_LLM_OPENROUTER_MODEL=stealth/ox-alpha
```

</details>

## License

Apache-2.0
