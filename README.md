<p align="center">
  <img src="https://avatars.githubusercontent.com/u/262526870?s=200" alt="EVOT" />
</p>

<p align="center">
  <strong>bendclaw — Self-Evolving AgentOS</strong>
</p>

<p align="center">
  One source of truth. All agents share it. Every agent evolves from it.
</p>

<p align="center">
  <a href="https://github.com/EvotAI/bendclaw/actions/workflows/ci.yml">
    <img src="https://github.com/EvotAI/bendclaw/actions/workflows/ci.yml/badge.svg" alt="CI" />
  </a>
  <a href="https://github.com/EvotAI/bendclaw/releases">
    <img src="https://img.shields.io/github/v/release/EvotAI/bendclaw" alt="Release" />
  </a>
  <a href="LICENSE">
    <img src="https://img.shields.io/badge/license-Apache--2.0-blue" alt="License" />
  </a>
</p>

---

## Why bendclaw

Traditional agent runtimes treat each agent as isolated — separate memory, separate context, separate mistakes. bendclaw is different.

**Single source of truth.** All agents read and write to one shared Databend store. Sessions, memories, learnings, skills, traces — one copy, shared by all.

**Self-evolving agents.** Every agent run produces learnings. These learnings are automatically injected into the prompt of every future run — across all agents. When one agent discovers a better approach, every agent inherits it.

**Stateless runtime.** bendclaw itself holds zero state. All persistence lives in Databend. Scale out to N instances, scale down to zero. No coordination needed.

---

## How It Works

```
 Client                    bendclaw                     Databend
   │                          │                            │
   │── POST /runs ───────────▶│                            │
   │                          │── load config, history ───▶│
   │                          │── load learnings ─────────▶│
   │                          │◀── prompt layers ──────────│
   │                          │                            │
   │◀── SSE: events ───────── │── agent loop ──────────────│
   │                          │   ┌─ LLM call             │
   │                          │   ├─ tool execution        │
   │                          │   ├─ trace recording ────▶│
   │                          │   └─ repeat until done     │
   │                          │                            │
   │                          │── persist run + events ──▶│
   │                          │── persist learnings ─────▶│
   │                          │                            │
   │                          │   Next run reads ─────────▶│
   │                          │   inherits all learnings ──│
```

### Prompt Layers

Every run assembles a system prompt from 8 layers, each loaded from the shared store:

```
Identity → Soul → System Prompt → Skills → Tools → Learnings → Recent Errors → Runtime
```

Learnings accumulate across all agents. Errors from recent runs are surfaced so agents avoid repeating mistakes.

---

## Architecture

```
                    Stateless — scale in/out freely
                                │
             ┌──────────────────┼──────────────────┐
             ▼                  ▼                  ▼
    ┌─────────────┐    ┌─────────────┐    ┌─────────────┐
    │  bendclaw   │    │  bendclaw   │    │  bendclaw   │
    │  ┌────────┐ │    │  ┌────────┐ │    │  ┌────────┐ │
    │  │Gateway │ │    │  │Gateway │ │    │  │Gateway │ │
    │  ├────────┤ │    │  ├────────┤ │    │  ├────────┤ │
    │  │ Kernel │ │    │  │ Kernel │ │    │  │ Kernel │ │
    │  └────────┘ │    │  └────────┘ │    │  └────────┘ │
    └──────┬──────┘    └──────┬──────┘    └──────┬──────┘
           └───────────────────┼───────────────────┘
                               ▼
              ┌─────────────────────────────────┐
              │            Databend              │
              │                                  │
              │  sessions · messages · memories  │
              │  learnings · skills · traces     │
              │  usage · config · variables      │
              │  tasks · feedback · run_events   │
              │                                  │
              │  One store. All agents share it. │
              └─────────────────────────────────┘
```

| Layer | Role |
|---|---|
| **Gateway** | HTTP routing, SSE streaming, Bearer auth, CORS |
| **Kernel** | Agent loop, LLM pool with failover, tools, context compaction |
| **Databend** | Single source of truth — all agent data lives here |

---

## Built-in Tools

| Category | Tools |
|---|---|
| **file** | `file_read`, `file_write`, `file_edit` |
| **shell** | `shell` — allowlisted commands with timeout |
| **memory** | `memory_write`, `memory_read`, `memory_search`, `memory_list`, `memory_delete` |
| **skill** | `skill_read` — documentation access |
| **databend** | `databend_query` — SQL against Databend |

---

## API

All endpoints served from `/v1`.

### Agents

| Endpoint | Method | Description |
|---|---|---|
| `/v1/agents` | GET | List agents |
| `/v1/agents/{agent_id}` | GET/DELETE | Get or delete agent |
| `/v1/agents/{agent_id}/setup` | POST | Create agent database |

### Sessions

| Endpoint | Method | Description |
|---|---|---|
| `/v1/agents/{agent_id}/sessions` | GET/POST | List or create sessions |
| `/v1/agents/{agent_id}/sessions/{session_id}` | GET/PUT/DELETE | Session CRUD |

### Runs

| Endpoint | Method | Description |
|---|---|---|
| `/v1/agents/{agent_id}/runs` | GET/POST | List runs or start a run (SSE) |
| `/v1/agents/{agent_id}/runs/{run_id}` | GET | Get run with events |
| `/v1/agents/{agent_id}/runs/{run_id}/continue` | POST | Continue paused run |
| `/v1/agents/{agent_id}/runs/{run_id}/cancel` | POST | Cancel run |
| `/v1/agents/{agent_id}/runs/{run_id}/events` | GET | List run events |
| `/v1/agents/{agent_id}/sessions/{session_id}/runs` | GET | Runs for session |

### Memories

| Endpoint | Method | Description |
|---|---|---|
| `/v1/agents/{agent_id}/memories` | GET/POST | List or create memories |
| `/v1/agents/{agent_id}/memories/{memory_id}` | GET/DELETE | Get or delete memory |
| `/v1/agents/{agent_id}/memories/search` | POST | Search memories |

### Learnings

| Endpoint | Method | Description |
|---|---|---|
| `/v1/agents/{agent_id}/learnings` | GET/POST | List or create learnings |
| `/v1/agents/{agent_id}/learnings/{learning_id}` | DELETE | Delete learning |
| `/v1/agents/{agent_id}/learnings/search` | POST | Search learnings |

### Skills

| Endpoint | Method | Description |
|---|---|---|
| `/v1/agents/{agent_id}/skills` | GET/POST | List or create skills |
| `/v1/agents/{agent_id}/skills/{skill_name}` | GET/DELETE | Get or delete skill |

### Config

| Endpoint | Method | Description |
|---|---|---|
| `/v1/agents/{agent_id}/config` | GET/PUT | Read or update config |
| `/v1/agents/{agent_id}/config/versions` | GET | List config versions |
| `/v1/agents/{agent_id}/config/versions/{version}` | GET | Get version |
| `/v1/agents/{agent_id}/config/rollback` | POST | Roll back config |

### Traces

| Endpoint | Method | Description |
|---|---|---|
| `/v1/agents/{agent_id}/traces` | GET | List traces |
| `/v1/agents/{agent_id}/traces/summary` | GET | Trace summary |
| `/v1/agents/{agent_id}/traces/{trace_id}` | GET | Get trace |
| `/v1/agents/{agent_id}/traces/{trace_id}/spans` | GET | List spans |

### Usage

| Endpoint | Method | Description |
|---|---|---|
| `/v1/agents/{agent_id}/usage` | GET | Usage summary |
| `/v1/agents/{agent_id}/usage/daily` | GET | Daily usage |
| `/v1/usage/summary` | GET | Global usage |

### Variables

| Endpoint | Method | Description |
|---|---|---|
| `/v1/agents/{agent_id}/variables` | GET/POST | List or create variables |
| `/v1/agents/{agent_id}/variables/{var_id}` | GET/PUT/DELETE | Variable CRUD |

### Tasks

| Endpoint | Method | Description |
|---|---|---|
| `/v1/agents/{agent_id}/tasks` | GET/POST | List or create tasks |
| `/v1/agents/{agent_id}/tasks/{task_id}` | PUT/DELETE | Update or delete task |
| `/v1/agents/{agent_id}/tasks/{task_id}/toggle` | POST | Toggle task |

### Feedback

| Endpoint | Method | Description |
|---|---|---|
| `/v1/agents/{agent_id}/feedback` | GET/POST | List or create feedback |
| `/v1/agents/{agent_id}/feedback/{feedback_id}` | DELETE | Delete feedback |

### Health

| Endpoint | Method | Description |
|---|---|---|
| `/health` | GET | Health check |

---

## Configuration

Three-layer merge: **config file** < **env vars** < **CLI args**.

| Env var | Config path | Description |
|---|---|---|
| `BENDCLAW_STORAGE_DATABEND_API_TOKEN` | `storage.databend_api_token` | **Required.** Databend API token |
| `BENDCLAW_AUTH_KEY` | `auth.api_key` | Bearer auth key (empty = auth disabled) |
| `BENDCLAW_SERVER_BIND_ADDR` | `server.bind_addr` | Listen address (default `127.0.0.1:8787`) |

See [`configs/bendclaw.toml.example`](configs/bendclaw.toml.example) for all options including LLM providers, workspace, and logging.

---

## Development

```bash
make setup    # install deps, hooks
make run      # start with dev config at localhost:8787
make test     # run all tests
make check    # fmt + clippy
```

First run creates `~/.bendclaw/bendclaw_dev.toml`. Configure LLM provider API keys and Databend credentials before use.

---

## License

Apache-2.0
