<p align="center">
  <img src="https://github.com/user-attachments/assets/132aa3cc-5c79-445a-8c18-5da152f7745d" alt="BendClaw" />
</p>

<p align="center">
  <strong>BendClaw</strong>
</p>

<p align="center">
  Distributed AI teams. Share everything. Co-evolve.
</p>

<p align="center">
  A Rust-native runtime where every execution produces reusable knowledge — no prompt engineering required.
</p>

<p align="center">
  The engine behind <a href="https://evot.ai">evot.ai</a>
</p>

---

## Why BendClaw

Most agent frameworks are stateless — every run starts from zero. BendClaw turns each execution into reusable knowledge that's automatically shared across the team and injected into future runs. Agents co-evolve without prompt engineering.

It's also a distributed runtime from day one. Agents collaborate across nodes, fan out subtasks, and collect results — all on a single shared data layer backed by Databend. Add nodes to scale; the cluster handles coordination.

## What's Inside

- **Autonomous learning** — agents extract and **share** knowledge from every run, auto-injected into future prompts for continuous **co-evolution**
- **Cluster dispatch** — agents **collaborate** across nodes, fan out subtasks, and collect results
- **Lease-based scheduling** — tasks and channel receivers claimed via distributed DB leases; automatic failover
- **Shared persistent memory** — vector + full-text search on shared cloud storage, one unified data layer
- **Hub integrations** — 100+ integrations (GitHub, Slack, Email, etc.) via pluggable skills
- **Secret-safe execution** — secrets in a vault, never exposed to LLMs; injected only at tool execution time
- **Full traceability** — spans, events, audits, sensitive field redaction; humans review, agents execute
- **Multi-tenant isolation** — separate DB per agent, isolated workspace per session
- **32 built-in tools** — file, search, shell, memory, recall, task, web, databend, channel, cluster
- **50+ REST endpoints** — SSE streaming, Bearer auth, per-agent scoping

## Quick Start

1. Go to [app.evot.ai](https://app.evot.ai/) → **Add AgentOS** → **Your Computer**

<table align="center">
  <tr>
    <td><a href="docs/console.png" target="_blank"><img src="docs/console.png" alt="Console" height="320" /></a></td>
    <td><a href="docs/chat.png" target="_blank"><img src="docs/chat.png" alt="Chat" height="320" /></a></td>
  </tr>
</table>

2. Copy and run the setup command:

```bash
curl -fsSL https://app.evot.ai/api/setup | sh -s -- <BASE64_CONFIG>
```

3. `bendclaw run` — the console detects your instance automatically

**CLI commands:**

| Command | Description |
|---------|-------------|
| `bendclaw run` | Run in foreground (default) |
| `bendclaw start` | Start server in background |
| `bendclaw stop` | Stop the server |
| `bendclaw restart` | Kill old process, start new one |
| `bendclaw status` | Show server status |
| `bendclaw update` | Upgrade to latest release |

> For development from source, see [Development](#development).

---


## Architecture

```
       HTTP / SSE                    HTTP / SSE                    HTTP / SSE
            │                             │                             │
            ▼                             ▼                             ▼
  ┌───────────────────┐         ┌───────────────────┐         ┌───────────────────┐
  │  BendClaw Node A  │         │  BendClaw Node B  │         │  BendClaw Node N  │
  │                   │         │                   │         │                   │
  │  Gateway          │         │  Gateway          │         │  Gateway          │
  │  Kernel + Hub     │ cluster │  Kernel + Hub     │ cluster │  Kernel + Hub     │
  │  Recall           │◄───────▶│  Recall           │◄───────▶│  Recall           │
  │  Lease            │   RPC   │  Lease            │   RPC   │  Lease            │
  │  Channels         │         │  Channels         │         │  Channels         │
  │                   │         │                   │         │                   │
  └─────────┬─────────┘         └─────────┬─────────┘         └─────────┬─────────┘
            └─────────────────────────────┼─────────────────────────────┘
                                          ▼
            ┌───────────────────────────────────────────────────────┐
            │                   Databend (Cloud)                    │
            │                                                       │
            │  sessions · runs · memories (vector + FTS)            │
            │  learnings · knowledge · skills · traces              │
            │  tasks · config · variables · feedback · channels     │
            │                                                       │
            │       Shared cloud storage. All agents,               │
            │              one data layer.                          │
            └───────────────────────────────────────────────────────┘
```

| Layer | Role |
|---|---|
| **Gateway** | HTTP routing, SSE streaming, Bearer auth, CORS, request logging |
| **Kernel** | Agent loop, LLM router (Anthropic / OpenAI) with circuit breaker and failover, tool registry, context compaction, prompt builder |
| **Recall** | Post-run knowledge extraction (fire-and-forget), learning accumulation, auto-injection into future prompts |
| **Lease** | Distributed lease coordination — claim/renew/release across nodes; per-resource callbacks for task scheduling and channel receiver lifecycle |
| **Hub** | Pluggable skill registry, auto-sync from remote repo, 100+ integrations fed into Kernel |
| **Channels** | Webhook ingestion (Feishu, Telegram, GitHub), HTTP API channel, lease-managed receivers, centralized sender trust (allow_from), inbound dispatch to Kernel |
| **Cluster** | Peer-to-peer RPC — node registration, heartbeat, autonomous subtask dispatch across nodes |
| **Databend** | Shared cloud storage — all agent data, one unified data layer |

---

## Built-in Tools

| Category | Tools | Description |
|---|---|---|
| **File** | `file_read`, `file_write`, `file_edit`, `list_dir` | Workspace file operations (sandbox mode optional) |
| **Search** | `grep`, `glob` | Content search (regex) and file search (glob pattern) across workspace |
| **Shell** | `shell` | Allowlisted commands with configurable timeout |
| **Memory** | `memory_write`, `memory_read`, `memory_search`, `memory_list`, `memory_delete` | Long-term memory with vector + full-text search |
| **Skill** | `skill_read`, `create_skill`, `remove_skill` | Skill documentation access and management; executable skills can act as runtime tools |
| **Recall** | `learning_write`, `learning_search`, `knowledge_search` | Agent self-improvement: write learnings, search accumulated knowledge |
| **Task** | `task_create`, `task_list`, `task_get`, `task_update`, `task_delete`, `task_toggle`, `task_history` | Cron task self-management |
| **Web** | `web_search`, `web_fetch` | Web search and page fetching |
| **Databend** | `databend` | SQL queries against the agent's Databend database |
| **Channel** | `channel_send` | Send messages through connected channels |
| **Cluster** | `cluster_nodes`, `cluster_dispatch`, `cluster_collect` | Discover peers, dispatch subtasks to other agents, collect results |
| **Coding** | `claude_code`, `codex_exec` | Delegate coding tasks to Claude Code or OpenAI Codex CLI with multi-turn session support |

---

## API

50+ REST endpoints with SSE streaming, Bearer auth, and per-agent scoping. See [docs/API.md](docs/API.md) for full reference.

---

## Development

```bash
make setup    # install protoc, git hooks
make check    # fmt + clippy
make test     # unit + integration + contract (no credentials needed)
make coverage   # generate HTML coverage report
```

---

## Community

- [GitHub Issues](https://github.com/EvotAI/bendclaw/issues) — bug reports & feature requests
- [Twitter @Evot_AI](https://twitter.com/Evot_AI) — updates & announcements
- team@evot.ai — reach the team directly

## License

Apache-2.0
