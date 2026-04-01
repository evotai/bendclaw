# Workbench V1: Session Replay

## Intent

Make sessions replayable through a single API call, by adding a thin semantic hint to the existing event stream and projecting a structured summary from raw facts.

Not a new harness. Not a second event store. Not an evaluation or diagnosis system.

## Module layout

Three files，三个职责：

```text
src/kernel/workbench/
  sem_event.rs        -- 采什么语义信号（SemEvent enum + capture helper）
  replay.rs           -- 怎么从原始事实组装 replay（load + project，同一文件两个函数）

src/service/v1/workbench/
  replay.rs           -- 怎么暴露给产品（HTTP, auth, ownership, 调 kernel）
```

加两个 mod.rs 做模块声明。不再拆更多文件。

## sem_event.rs

定义 `SemEvent` 和一个 capture helper：

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind")]
pub enum SemEvent {
    CapabilitiesSnapshot {
        tools: Vec<String>,
        skills: Vec<String>,
    },
}

impl SemEvent {
    pub fn name(&self) -> &'static str {
        match self {
            Self::CapabilitiesSnapshot { .. } => "sem.capabilities_snapshot",
        }
    }
}

/// Build a CapabilitiesSnapshot event from the tool schemas and skill names
/// available at run start.
pub fn capture_capabilities(tools: &[ToolSchema], skills: &[String]) -> Event {
    Event::Semantic(SemEvent::CapabilitiesSnapshot {
        tools: tools.iter().map(|t| t.function.name.clone()).collect(),
        skills: skills.to_vec(),
    })
}
```

Run-level granularity。记录 `PromptBuilder` 组装的完整工具/技能集，不是 per-turn 的 progressive 子集。

`Event` enum 加一个变体：

```rust
// src/kernel/run/event.rs
Event::Semantic(SemEvent)
```

`Event::name()` 对 `Semantic` 变体委托给 `SemEvent::name()`。

### 事件流

不引入新写入路径。和所有其他事件走同一条路：

1. `SessionRunCoordinator::start` 里，`PromptBuilder::build()` 完成后，调 `capture_capabilities(...)` 构造事件
2. Push 到 `initial_events` vec（和 `run.started`、`prompt.built` 并列）
3. `TurnPersister::build_event_records` 序列化为 `RunEventRecord`
4. 通过现有 `PersistOp` 写入 `run_events`

## replay.rs

同一个文件，两个函数，IO 和投影分开：

```rust
/// IO: 从存储加载 replay 所需的原始事实
pub async fn load_replay_facts(
    store: &AgentStore,
    session_id: &str,
) -> Result<ReplayFacts>

/// 纯逻辑: 从原始事实投影出 replay summary
pub fn project_replay(facts: ReplayFacts) -> SessionReplaySummary
```

### ReplayFacts

```rust
pub struct ReplayFacts {
    pub runs: Vec<RunRecord>,
    pub events: Vec<RunEventRecord>,
}
```

### load_replay_facts

1. `RunRepo::list_by_session(session_id)` — 返回 `created_at DESC`
2. `RunEventRepo::list_by_session(session_id)` — 新方法，按 `seq ASC, created_at ASC` 返回，无过滤

### project_replay

纯函数，无 IO：

1. 将 runs 反转为时间正序
2. 遍历 events 一次，跳过 `StreamDelta`：
   - 从 `ToolStart`/`ToolEnd` 提取 tool timeline
   - 从 `sem.capabilities_snapshot` 提取 capabilities，按 `run_id` 归组
3. 从 `RunRecord` 字段构建 run summaries
4. 从时间正序最后一个 run 的 `status`/`stop_reason`/`error` 派生 outcome

### 输出类型

```rust
pub struct SessionReplaySummary {
    pub session_id: String,
    pub runs: Vec<RunSummary>,
    pub tool_timeline: Vec<ToolTimelineEntry>,
    pub capabilities_by_run: Vec<RunCapabilities>,
    pub outcome: OutcomeSummary,
}

pub struct RunSummary {
    pub run_id: String,
    pub status: String,
    pub stop_reason: String,
    pub iterations: u32,
    pub duration_ms: u64,
    pub error: String,
}

pub struct ToolTimelineEntry {
    pub run_id: String,
    pub seq: u32,
    pub tool_call_id: String,
    pub name: String,
    pub success: bool,
    pub duration_ms: Option<u64>,
}

pub struct RunCapabilities {
    pub run_id: String,
    pub tools: Vec<String>,
    pub skills: Vec<String>,
}

pub struct OutcomeSummary {
    pub final_status: String,
    pub final_stop_reason: String,
    pub error: Option<String>,
}
```

## service/v1/workbench/replay.rs

`GET /v1/agents/{agent_id}/workbench/sessions/{session_id}/replay`

- `agent_id` 从 path 取，走 `agent_pool(agent_id)` 拿库
- 现有 auth middleware + `RequestContext`
- 校验 session ownership（agent_id + user_id）
- 调 `load_replay_facts` → `project_replay` → 返回 JSON

## 不做的事

- 不拆 evaluation/diagnosis/improvement
- 不引入新 store/repo 抽象
- 不把 replay 拆成更多小文件
- 不设计 trace 融合、归因 DSL、规则引擎
- 不做 per-turn capability tracking
- 不做 actual_path / task_type
- 不加新数据库表或迁移
- 不做缓存或物化

## 改动清单

1. `src/kernel/workbench/sem_event.rs` — SemEvent + capture_capabilities
2. `src/kernel/workbench/replay.rs` — ReplayFacts, load_replay_facts, project_replay, 输出类型
3. `src/kernel/workbench/mod.rs` — 模块声明
4. `src/kernel/run/event.rs` — 加 `Event::Semantic(SemEvent)` + name() 委托
5. `src/kernel/session/run.rs` — initial_events 里 push CapabilitiesSnapshot
6. `src/storage/dal/run_event/repo.rs` — 加 `list_by_session` 方法
7. `src/service/v1/workbench/replay.rs` — HTTP handler
8. `src/service/v1/workbench/mod.rs` — 路由声明
9. `src/service/router.rs` — 注册 workbench 路由
10. Tests: SemEvent 序列化, project_replay 纯函数测试（合成 fixtures）, API auth
