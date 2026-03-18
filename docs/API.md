# BendClaw API Reference

All endpoints are under `/v1`.

- Agent APIs are scoped by `agent_id`
- Most request flows also require `x-user-id`
- `Authorization: Bearer <key>` is required except for `/health` and channel webhooks

## Agents

| Endpoint | Method | Description |
|---|---|---|
| `/v1/agents` | GET | List agents |
| `/v1/agents/{agent_id}` | GET / DELETE | Get or delete agent |
| `/v1/agents/{agent_id}/setup` | POST | Create agent database |

## Sessions

| Endpoint | Method | Description |
|---|---|---|
| `/v1/agents/{agent_id}/sessions` | GET / POST | List or create sessions |
| `/v1/agents/{agent_id}/sessions/{session_id}` | GET / PUT / DELETE | Session CRUD |

## Runs

| Endpoint | Method | Description |
|---|---|---|
| `/v1/agents/{agent_id}/runs` | GET / POST | List runs or start a run (JSON or SSE) |
| `/v1/agents/{agent_id}/runs/{run_id}` | GET | Get run with events |
| `/v1/agents/{agent_id}/runs/{run_id}/cancel` | POST | Cancel run |
| `/v1/agents/{agent_id}/runs/{run_id}/continue` | POST | Continue paused run |
| `/v1/agents/{agent_id}/runs/{run_id}/events` | GET | List run events |
| `/v1/agents/{agent_id}/sessions/{session_id}/runs` | GET | Runs for session |

## Memories

| Endpoint | Method | Description |
|---|---|---|
| `/v1/agents/{agent_id}/memories` | GET / POST | List or create memories |
| `/v1/agents/{agent_id}/memories/{memory_id}` | GET / DELETE | Get or delete memory |
| `/v1/agents/{agent_id}/memories/search` | POST | Semantic + full-text search |

## Learnings

| Endpoint | Method | Description |
|---|---|---|
| `/v1/agents/{agent_id}/learnings` | GET / POST | List or create learnings |
| `/v1/agents/{agent_id}/learnings/{learning_id}` | GET / DELETE | Get or delete learning |
| `/v1/agents/{agent_id}/learnings/search` | POST | Search learnings |

## Knowledge

| Endpoint | Method | Description |
|---|---|---|
| `/v1/agents/{agent_id}/knowledge` | GET / POST | List or create knowledge entries |
| `/v1/agents/{agent_id}/knowledge/{knowledge_id}` | GET / DELETE | Get or delete knowledge |
| `/v1/agents/{agent_id}/knowledge/search` | POST | Search knowledge |

## Skills

| Endpoint | Method | Description |
|---|---|---|
| `/v1/agents/{agent_id}/skills` | GET / POST | List or create skills |
| `/v1/agents/{agent_id}/skills/{skill_name}` | GET / DELETE | Get or delete skill |

## Hub

| Endpoint | Method | Description |
|---|---|---|
| `/v1/hub/skills` | GET | List available hub skills |
| `/v1/hub/skills/{skill_name}/credentials` | GET | Required credentials for a skill |
| `/v1/hub/status` | GET | Hub sync status |

## Config

| Endpoint | Method | Description |
|---|---|---|
| `/v1/agents/{agent_id}/config` | GET / PUT | Read or update config |
| `/v1/agents/{agent_id}/config/versions` | GET | List config versions |
| `/v1/agents/{agent_id}/config/versions/{version}` | GET | Get specific version |
| `/v1/agents/{agent_id}/config/rollback` | POST | Roll back to a version |

## Traces

| Endpoint | Method | Description |
|---|---|---|
| `/v1/agents/{agent_id}/traces` | GET | List traces |
| `/v1/agents/{agent_id}/traces/summary` | GET | Trace summary |
| `/v1/agents/{agent_id}/traces/{trace_id}` | GET | Get trace |
| `/v1/agents/{agent_id}/traces/{trace_id}/spans` | GET | List spans |
| `/v1/agents/{agent_id}/traces/{trace_id}/children` | GET | List child traces (distributed) |

## Usage

| Endpoint | Method | Description |
|---|---|---|
| `/v1/agents/{agent_id}/usage` | GET | Agent usage summary |
| `/v1/agents/{agent_id}/usage/daily` | GET | Daily usage breakdown |
| `/v1/usage/summary` | GET | Global usage across all agents |

## Variables

| Endpoint | Method | Description |
|---|---|---|
| `/v1/agents/{agent_id}/variables` | GET / POST | List or create variables |
| `/v1/agents/{agent_id}/variables/{var_id}` | GET / PUT / DELETE | Variable CRUD |

## Tasks

| Endpoint | Method | Description |
|---|---|---|
| `/v1/agents/{agent_id}/tasks` | GET / POST | List or create scheduled tasks |
| `/v1/agents/{agent_id}/tasks/{task_id}` | PUT / DELETE | Update or delete task |
| `/v1/agents/{agent_id}/tasks/{task_id}/toggle` | POST | Enable or disable task |
| `/v1/agents/{agent_id}/tasks/{task_id}/history` | GET | Task execution history |

## Feedback

| Endpoint | Method | Description |
|---|---|---|
| `/v1/agents/{agent_id}/feedback` | GET / POST | List or create feedback |
| `/v1/agents/{agent_id}/feedback/{feedback_id}` | DELETE | Delete feedback |

## Channels

| Endpoint | Method | Description |
|---|---|---|
| `/v1/agents/{agent_id}/channels/accounts` | GET / POST | List or create channel accounts |
| `/v1/agents/{agent_id}/channels/accounts/{account_id}` | GET / DELETE | Get or delete channel account |
| `/v1/agents/{agent_id}/channels/messages` | GET | List channel messages |
| `/v1/agents/{agent_id}/channels/webhook/{account_id}` | POST | Receive inbound webhook (no auth) |

## Stats & Health

| Endpoint | Method | Description |
|---|---|---|
| `/health` | GET | Health check |
| `/v1/stats/sessions` | GET | Active session stats |
| `/v1/stats/can_suspend` | GET | Whether the instance can suspend |
