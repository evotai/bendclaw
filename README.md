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
- **30 built-in tools** — file, shell, memory, recall, task, web, databend, channel, cluster
- **50+ REST endpoints** — SSE streaming, Bearer auth, per-agent scoping

## Quick Start

1. Go to [evot.ai](https://evot.ai) → **AgentOS** → **Add AgentOS** → **Your Computer**
2. Copy and run the setup command:

```bash
curl -fsSL https://app.evot.ai/api/setup | sh -s -- <BASE64_CONFIG>
```

3. `bendclaw run` — the console detects your instance automatically

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
| **Channels** | Webhook ingestion (Feishu, Telegram, GitHub), lease-managed receivers, centralized sender trust (allow_from), inbound dispatch to Kernel |
| **Cluster** | Peer-to-peer RPC — node registration, heartbeat, autonomous subtask dispatch across nodes |
| **Databend** | Shared cloud storage — all agent data, one unified data layer |

---

## Built-in Tools

| Category | Tools | Description |
|---|---|---|
| **File** | `file_read`, `file_write`, `file_edit`, `list_dir` | Workspace file operations (sandbox mode optional) |
| **Shell** | `shell` | Allowlisted commands with configurable timeout |
| **Memory** | `memory_write`, `memory_read`, `memory_search`, `memory_list`, `memory_delete` | Long-term memory with vector + full-text search |
| **Skill** | `skill_read`, `create_skill`, `remove_skill` | Skill documentation access and management; executable skills can act as runtime tools |
| **Recall** | `learning_write`, `learning_search`, `knowledge_search` | Agent self-improvement: write learnings, search accumulated knowledge |
| **Task** | `task_create`, `task_list`, `task_get`, `task_update`, `task_delete`, `task_toggle`, `task_history` | Cron task self-management |
| **Web** | `web_search`, `web_fetch` | Web search and page fetching |
| **Databend** | `databend` | SQL queries against the agent's Databend database |
| **Channel** | `channel_send` | Send messages through connected channels |
| **Cluster** | `cluster_nodes`, `cluster_dispatch`, `cluster_collect` | Discover peers, dispatch subtasks to other agents, collect results |

---

## API

All endpoints are under `/v1`.

- Agent APIs are scoped by `agent_id`
- Most request flows also require `x-user-id`
- `Authorization: Bearer <key>` is required except for `/health` and channel webhooks

<details>
<summary>Agents</summary>

| Endpoint | Method | Description |
|---|---|---|
| `/v1/agents` | GET | List agents |
| `/v1/agents/{agent_id}` | GET / DELETE | Get or delete agent |
| `/v1/agents/{agent_id}/setup` | POST | Create agent database |

</details>

<details>
<summary>Sessions</summary>

| Endpoint | Method | Description |
|---|---|---|
| `/v1/agents/{agent_id}/sessions` | GET / POST | List or create sessions |
| `/v1/agents/{agent_id}/sessions/{session_id}` | GET / PUT / DELETE | Session CRUD |

</details>

<details>
<summary>Runs</summary>

| Endpoint | Method | Description |
|---|---|---|
| `/v1/agents/{agent_id}/runs` | GET / POST | List runs or start a run (JSON or SSE) |
| `/v1/agents/{agent_id}/runs/{run_id}` | GET | Get run with events |
| `/v1/agents/{agent_id}/runs/{run_id}/cancel` | POST | Cancel run |
| `/v1/agents/{agent_id}/runs/{run_id}/continue` | POST | Continue paused run |
| `/v1/agents/{agent_id}/runs/{run_id}/events` | GET | List run events |
| `/v1/agents/{agent_id}/sessions/{session_id}/runs` | GET | Runs for session |

</details>

<details>
<summary>Memories</summary>

| Endpoint | Method | Description |
|---|---|---|
| `/v1/agents/{agent_id}/memories` | GET / POST | List or create memories |
| `/v1/agents/{agent_id}/memories/{memory_id}` | GET / DELETE | Get or delete memory |
| `/v1/agents/{agent_id}/memories/search` | POST | Semantic + full-text search |

</details>

<details>
<summary>Learnings</summary>

| Endpoint | Method | Description |
|---|---|---|
| `/v1/agents/{agent_id}/learnings` | GET / POST | List or create learnings |
| `/v1/agents/{agent_id}/learnings/{learning_id}` | GET / DELETE | Get or delete learning |
| `/v1/agents/{agent_id}/learnings/search` | POST | Search learnings |

</details>

<details>
<summary>Knowledge</summary>

| Endpoint | Method | Description |
|---|---|---|
| `/v1/agents/{agent_id}/knowledge` | GET / POST | List or create knowledge entries |
| `/v1/agents/{agent_id}/knowledge/{knowledge_id}` | GET / DELETE | Get or delete knowledge |
| `/v1/agents/{agent_id}/knowledge/search` | POST | Search knowledge |

</details>

<details>
<summary>Skills</summary>

| Endpoint | Method | Description |
|---|---|---|
| `/v1/agents/{agent_id}/skills` | GET / POST | List or create skills |
| `/v1/agents/{agent_id}/skills/{skill_name}` | GET / DELETE | Get or delete skill |

</details>

<details>
<summary>Hub</summary>

| Endpoint | Method | Description |
|---|---|---|
| `/v1/hub/skills` | GET | List available hub skills |
| `/v1/hub/skills/{skill_name}/credentials` | GET | Required credentials for a skill |
| `/v1/hub/status` | GET | Hub sync status |

</details>

<details>
<summary>Config</summary>

| Endpoint | Method | Description |
|---|---|---|
| `/v1/agents/{agent_id}/config` | GET / PUT | Read or update config |
| `/v1/agents/{agent_id}/config/versions` | GET | List config versions |
| `/v1/agents/{agent_id}/config/versions/{version}` | GET | Get specific version |
| `/v1/agents/{agent_id}/config/rollback` | POST | Roll back to a version |

</details>

<details>
<summary>Traces</summary>

| Endpoint | Method | Description |
|---|---|---|
| `/v1/agents/{agent_id}/traces` | GET | List traces |
| `/v1/agents/{agent_id}/traces/summary` | GET | Trace summary |
| `/v1/agents/{agent_id}/traces/{trace_id}` | GET | Get trace |
| `/v1/agents/{agent_id}/traces/{trace_id}/spans` | GET | List spans |
| `/v1/agents/{agent_id}/traces/{trace_id}/children` | GET | List child traces (distributed) |

</details>

<details>
<summary>Usage</summary>

| Endpoint | Method | Description |
|---|---|---|
| `/v1/agents/{agent_id}/usage` | GET | Agent usage summary |
| `/v1/agents/{agent_id}/usage/daily` | GET | Daily usage breakdown |
| `/v1/usage/summary` | GET | Global usage across all agents |

</details>

<details>
<summary>Variables</summary>

| Endpoint | Method | Description |
|---|---|---|
| `/v1/agents/{agent_id}/variables` | GET / POST | List or create variables |
| `/v1/agents/{agent_id}/variables/{var_id}` | GET / PUT / DELETE | Variable CRUD |

</details>

<details>
<summary>Tasks</summary>

| Endpoint | Method | Description |
|---|---|---|
| `/v1/agents/{agent_id}/tasks` | GET / POST | List or create scheduled tasks |
| `/v1/agents/{agent_id}/tasks/{task_id}` | PUT / DELETE | Update or delete task |
| `/v1/agents/{agent_id}/tasks/{task_id}/toggle` | POST | Enable or disable task |
| `/v1/agents/{agent_id}/tasks/{task_id}/history` | GET | Task execution history |

</details>

<details>
<summary>Feedback</summary>

| Endpoint | Method | Description |
|---|---|---|
| `/v1/agents/{agent_id}/feedback` | GET / POST | List or create feedback |
| `/v1/agents/{agent_id}/feedback/{feedback_id}` | DELETE | Delete feedback |

</details>

<details>
<summary>Channels</summary>

| Endpoint | Method | Description |
|---|---|---|
| `/v1/agents/{agent_id}/channels/accounts` | GET / POST | List or create channel accounts |
| `/v1/agents/{agent_id}/channels/accounts/{account_id}` | GET / DELETE | Get or delete channel account |
| `/v1/agents/{agent_id}/channels/messages` | GET | List channel messages |
| `/v1/agents/{agent_id}/channels/webhook/{account_id}` | POST | Receive inbound webhook (no auth) |

</details>

<details>
<summary>Stats & Health</summary>

| Endpoint | Method | Description |
|---|---|---|
| `/health` | GET | Health check |
| `/v1/stats/sessions` | GET | Active session stats |
| `/v1/stats/can_suspend` | GET | Whether the instance can suspend |

</details>

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

- [GitHub Issues](https://github.com/datafuselabs/evot-ai/issues) — bug reports & feature requests
- [Twitter @Evot_AI](https://twitter.com/Evot_AI) — updates & announcements
- team@evot.ai — reach the team directly

## License

Apache-2.0
