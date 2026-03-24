# Active Turn Revision Design

## Goal

When a new user message arrives while the same session already has an active run:

- do not hard-reject immediately
- do not always ask the user to choose
- recognize when the user is revising the current task
- restart safely when the task scope changes
- only ask when the relation is ambiguous

The design must stay:

- clear: execution and coordination separated
- decoupled: no overlap policy inside the agent engine
- easy to test: policy logic testable without LLM, tools, or streaming runtime

## Real Target Scenario

Example:

1. user: "please clean databases prefixed with `test_`"
2. agent starts a long-running cleanup
3. user: "also need to clean databases prefixed with `xx_`"

Expected behavior:

- this is not a normal follow-up
- this is not a new unrelated task
- this is a revision of the active destructive task
- system should cancel current run and restart with the revised scope

Expected user-facing response:

- "Received. I stopped the current cleanup and am restarting with prefixes `test_` and `xx_`."

System should not first ask:

- "Do you want to continue or cancel?"

Because the user already expressed the new desired scope.

## Current Baseline

`bendclaw` already has the right execution primitive:

- `Session` owns one active run
- `SessionState::Running { run_id, cancel, ... }`
- `cancel_current()` / `cancel_run(run_id)`
- `Stream` persists success / cancelled / error

Today, `Session::run()` rejects if a run is already active.

This is good and should remain true.

`Session` should stay as the single-run execution boundary.

## Design Principle

Add one thin layer before `session.run()`:

- `Session` remains execution-only
- new coordination layer handles overlap between incoming turns and active runs

Do not turn `Session` into:

- a queue manager
- a clarification manager
- an intent classifier

## Core Idea

Overlap handling should be relation-first, not decision-first.

When a message arrives during an active run, classify its relation to the active task:

- `Append`
- `Revise`
- `ForkOrAsk`

Only `ForkOrAsk` should ask the user.

## Relation Semantics

### `Append`

The new message adds detail but does not materially change the task boundary.

Examples:

- "also save the result to a file"
- "use markdown output"
- "after that, summarize the failures"

Suggested action:

- keep current run
- append/collect as follow-up

### `Revise`

The new message changes target scope, constraints, filters, or goal shape of the active task.

Examples:

- "also need to clean `xx_` prefixed databases"
- "do not delete, only list them"
- "only operate in staging, not production"
- "exclude system databases"

Suggested action:

- cancel current run
- restart from new user message

### `ForkOrAsk`

The relation to the current task is unclear or looks like a separate task.

Examples:

- "also check warehouse slow queries"
- "this does not look right, handle that one too"
- "look into the issue from this morning"

Suggested action:

- ask the user what they mean

## Proposed Components

### 1. `TurnCoordinator`

Responsibility:

- single entry for all new user turns
- if no active run: start immediately
- if active run: route by relation
- own state transitions after classification or decision

Suggested file:

- `src/kernel/runtime/turn_coordinator.rs`

### 2. `RunSnapshotStore`

Responsibility:

- maintain a compact structured summary of the currently active run
- expose exactly the data overlap classification needs

Suggested file:

- `src/kernel/runtime/run_snapshot.rs`

Why:

- overlap logic should not depend on full transcript parsing
- avoid feeding the entire session history into every overlap decision

### 3. `TurnRelationClassifier`

Responsibility:

- classify new message relative to active run
- output `Append | Revise | ForkOrAsk`

Suggested file:

- `src/kernel/runtime/turn_relation.rs`

Important:

- system trusts the relation enum
- system does not trust freeform wording for execution semantics

### 4. `PendingDecisionStore`

Responsibility:

- store one unresolved clarification per session
- separate from `SessionState`
- only used for ambiguous overlap

Suggested file:

- `src/kernel/runtime/pending_decision.rs`

### 5. `Clarifier`

Responsibility:

- phrase a natural-language question only when relation is ambiguous
- no tools
- no side effects
- no session mutation

Suggested file:

- `src/kernel/runtime/clarifier.rs`

### 6. `DecisionResolver`

Responsibility:

- parse the user's clarification reply into:
  - `ContinueCurrent`
  - `CancelAndSwitch`
  - `AppendAsFollowup`

Suggested file:

- `src/kernel/runtime/decision_resolver.rs`

## Boundaries

### Keep in `Session`

- run creation
- cancellation token ownership
- real run history mutation
- real run event streaming
- run persistence

### Keep out of `Session`

- overlap classification
- pending user decisions
- follow-up buffering
- clarification generation
- user reply resolution

## Core Types

```rust
pub enum RunRisk {
    ReadOnly,
    Mutating,
    Destructive,
}

pub enum TurnRelation {
    Append,
    Revise,
    ForkOrAsk,
}

pub struct RunSnapshot {
    pub session_id: String,
    pub run_id: String,
    pub summary: String,
    pub risk: RunRisk,
    pub target_scope: Option<String>,
    pub started_at: std::time::Instant,
}

pub enum DecisionOption {
    ContinueCurrent,
    CancelAndSwitch,
    AppendAsFollowup,
}

pub struct PendingDecision {
    pub session_id: String,
    pub active_run_id: String,
    pub question_id: String,
    pub question_text: String,
    pub candidate_input: String,
    pub options: Vec<DecisionOption>,
    pub created_at: std::time::Instant,
}

pub enum SubmitTurnResult {
    StartedRun { run_id: String, stream: crate::kernel::session::session_stream::Stream },
    RevisedAndRestarted { old_run_id: String, new_run_id: String, stream: crate::kernel::session::session_stream::Stream },
    WaitingForDecision { question_id: String },
    AppendedToFollowupQueue,
}
```

## Event Model

Use the existing event system.

Add:

```rust
TaskRevised {
    previous_run_id: String,
    message: String,
}

DecisionRequired {
    question_id: String,
    message: String,
    options: Vec<String>,
}
```

Optional later:

```rust
DecisionResolved {
    question_id: String,
    selected: String,
}
```

This keeps:

- SSE aligned
- API aligned
- channel delivery aligned

## Runtime Flow

### Case 1: No Active Run

1. inbound message arrives
2. `TurnCoordinator::submit_turn(...)`
3. no active run
4. call `session.run(...)`
5. return `StartedRun`

### Case 2: Active Run + `Append`

1. inbound message arrives
2. load `RunSnapshot`
3. classify relation as `Append`
4. store one buffered follow-up payload for the session
5. let active run continue
6. start follow-up turn after current run completes

### Case 3: Active Run + `Revise`

1. inbound message arrives
2. load `RunSnapshot`
3. classify relation as `Revise`
4. call `session.cancel_current()`
5. wait for current run to reach terminal state
6. emit `TaskRevised`
7. start a fresh run using the new input
8. return `RevisedAndRestarted`

This is the main path for scope changes of destructive tasks.

### Case 4: Active Run + `ForkOrAsk`

1. inbound message arrives
2. load `RunSnapshot`
3. classify relation as `ForkOrAsk`
4. call `Clarifier`
5. store `PendingDecision`
6. emit `DecisionRequired`
7. do not start new run yet

### Case 5: Awaiting Decision

If a session already has a pending decision:

1. incoming message is treated as decision reply
2. `DecisionResolver` parses it
3. system executes:
   - continue current
   - cancel and switch
   - append as follow-up

## Why Relation-First Is Better

If we always ask during overlap:

- user experience is slower
- destructive-task revisions become awkward
- natural corrections feel robotic

If we always auto-cancel:

- harmless follow-ups kill useful work
- users lose ongoing progress

Relation-first gives the right default:

- append when it is clearly append
- revise when it is clearly revise
- ask only when unclear

## Run Snapshot Strategy

Do not classify overlap from raw full history.

Use a compact snapshot:

- current task summary
- target scope summary
- run risk
- optional recent operation summary

Example snapshot for the cleanup scenario:

```json
{
  "summary": "clean databases with prefix test_",
  "risk": "destructive",
  "target_scope": "prefix=test_"
}
```

New user message:

- "also need to clean databases prefixed with xx_"

Expected classification:

- `Revise`

Reason:

- scope of the same destructive task changed

## LLM Usage Rule

LLM may help in two places:

- relation classification
- clarification wording

LLM must not own execution semantics.

The system owns:

- whether to cancel
- whether to queue
- whether to ask

Recommended trust boundary:

- trust structured `relation`
- ignore freeform prose for execution control

## Suggested Public Interface

Add one runtime entry:

```rust
pub async fn submit_turn(
    &self,
    agent_id: &str,
    session_id: &str,
    user_id: &str,
    input: String,
    trace_id: &str,
    parent_run_id: Option<&str>,
    parent_trace_id: &str,
    origin_node_id: &str,
    is_remote_dispatch: bool,
) -> Result<SubmitTurnResult>
```

Migrate these callers:

- `src/service/v1/runs/service.rs`
- `src/kernel/channel/dispatch.rs`

They should stop calling `session.run(...)` directly.

## Follow-up Buffer Scope

For v1:

- at most one buffered follow-up payload per session
- merge by appending text with `\n\n`

This is enough for:

- append semantics
- simple operator experience

No generic queue system needed yet.

## Storage Choice

For v1:

- keep `PendingDecision` in memory
- keep follow-up buffer in memory
- keep `RunSnapshot` in memory

Reason:

- no migration
- simpler
- enough to validate behavior

## Failure Semantics

### Classifier failure

Fallback to `ForkOrAsk`.

Fail safe.

### Clarifier failure

Fallback to deterministic template:

- "I am still working on the previous task. Do you want me to continue it, replace it with your new request, or handle your new message after this run?"

### Unclear decision reply

Ask again.

Do not guess.

### Active run ends before user replies

Then decision reply resolves against idle session:

- `CancelAndSwitch` => start new run
- `AppendAsFollowup` => start new run
- `ContinueCurrent` => no-op

## Test Strategy

### 1. Pure Unit Tests

Target:

- `TurnRelationClassifier` fake/deterministic implementation
- `DecisionResolver`
- follow-up merge helper
- relation-to-action transition logic

No tokio, no LLM, no session.

### 2. Coordinator Tests

Inject fakes:

- fake session handle
- fake classifier
- fake clarifier
- fake event emitter

Verify:

- idle -> starts run
- running + append -> buffers
- running + revise -> cancels then restarts
- running + fork -> emits `DecisionRequired`
- awaiting decision + switch -> cancels then restarts

### 3. Integration Tests

API and channel entrypoints:

- second message during active run no longer returns "session already has a running run"
- revise scenario restarts the run
- ambiguous scenario produces clarification event

### 4. Scenario Tests

Table-driven tests for real operator cases:

- active: "clean `test_` databases", new: "also clean `xx_`" => `Revise`
- active: "list `test_` databases", new: "also include `xx_`" => `Append` or `Revise` depending on policy
- active: "clean `test_` databases", new: "check warehouse slowness" => `ForkOrAsk`

## Rollout Plan

### Phase 1

- deterministic `RunSnapshot`
- deterministic or stubbed relation classifier
- in-memory pending decision store
- in-memory follow-up buffer
- `Revise` path wired end-to-end
- `ForkOrAsk` uses template clarifier

### Phase 2

- optional LLM-powered relation classifier
- optional LLM-powered natural clarifier wording
- richer per-channel UX

### Phase 3

- persisted pending decisions
- richer buffered follow-up semantics

## Non-Goals

Not included here:

- tool-boundary injection / steer
- full generic queue scheduler
- multiple concurrent runs per session
- letting the LLM decide cancel/continue directly

## Why This Fits `bendclaw`

It matches current architecture:

- `Session` already models one active run cleanly
- runtime already owns session lookup and commands
- event stream already exists
- cancel semantics already exist

The new layer stays:

- small
- explicit
- testable
- aligned with real task-revision behavior
