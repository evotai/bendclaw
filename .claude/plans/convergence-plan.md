# Convergence Plan — Final Architecture Cleanup

## Summary

Move local assembly logic back into `kernel/session/assembly/local.rs`, strip `src/local/` down to CLI-only, fix JsonSessionStore double-nesting, wire SkillRunner into cloud, remove AgentSource::Local from invocation, clean all residual code.

## Phase 1: Move local assembly into kernel, strip src/local/

**Move** `src/local/assemble.rs` → `src/kernel/session/assembly/local.rs`
- LocalBuildOptions stays here
- LocalRuntimeDeps becomes a private helper struct inside this file (no longer public)
- `from_runtime()` stays as a constructor

**Move** `src/local/runtime.rs` content into `src/kernel/session/assembly/local.rs` as internal struct

**Delete** `src/local/assemble.rs` and `src/local/runtime.rs`

**Update** `src/local/cmd_run.rs`:
- Import from `kernel::session::assembly::local` instead of `local::assemble`
- Construct LocalRuntimeDeps via the assembly module

**Update** `src/local/mod.rs` — only `args.rs` and `cmd_run.rs`

**Update** `src/kernel/session/assembly/mod.rs` — add `pub mod local`

## Phase 2: Remove AgentSource::Local from invocation

Choose **方案 B** — invocation is cloud-only, bendclaw-local bypasses it entirely.

**Delete** `AgentSource::Local` variant from `src/kernel/invocation/request.rs`
**Delete** the local branch from `src/kernel/invocation/session_route.rs`
**Remove** all `use crate::local::*` from invocation
**Update** `AgentSource` — if only Cloud remains, simplify or remove the enum

Verify: `grep -rn 'crate::local' src/kernel/` returns empty.

## Phase 3: Fix JsonSessionStore — session_root semantics, no double-nesting

Current: `base_dir/{session_id}/session.json` — double-nests when base_dir already contains session_id.

**Change** JsonSessionStore to session_root semantics:
```
{base_dir}/
  session.json
  runs/{run_id}.json
  events/{run_id}.jsonl
  usage/{run_id}.json
```

- Remove `session_dir()` method — all paths derive from `base_dir` directly
- `session_file()` → `base_dir/session.json`
- `run_file(run_id)` → `base_dir/runs/{run_id}.json`
- `events_file(run_id)` → `base_dir/events/{run_id}.jsonl`
- `usage_file(run_id)` → `base_dir/usage/{run_id}.json`
- All SessionStore methods that take `session_id` ignore it for path computation (the store is already scoped to one session)

**Update** local assembly:
```
session_root = config.workspace.session_dir(user_id, agent_id, session_id)
workspace.dir = session_root  (already the case via build_workspace)
workspace.cwd = user-specified or session_root
store = JsonSessionStore::new(session_root)
```

No double nesting. Workspace and store share the same root.

**Update** json_store tests to match new layout.

## Phase 4: Wire SkillRunner into cloud assembly

**In** `src/kernel/session/assembly/cloud.rs`:
```rust
let skill_executor: Arc<dyn SkillExecutor> = Arc::new(SkillRunner::new(
    agent_id,
    user_id,
    self.runtime.org.skills().clone(),
    workspace.clone(),
    pool.clone(),
));
```

Replace the current `Arc::new(NoopSkillExecutor)`.

**Local assembly** keeps `NoopSkillExecutor` — local has no skill service.

## Phase 5: Verify Session::from_assembly() is pure mapping

Already done — `from_assembly()` passes through `skill_executor` directly. Verify no defaults/overrides remain.

## Phase 6: Clean residual code

- Remove unused imports across all touched files
- Delete `src/cli/cmd_agent.rs` if still on disk (already done)
- Verify `src/kernel/agent_store/store.rs` has no SessionStore bridge (already done)
- Remove any `#[allow(dead_code)]` on code that's actually dead
- Run `cargo check` for zero warnings

## Phase 7: Tests

1. **Local assembly directory layout** — verify no double-nesting, workspace.dir and store share root
2. **Cloud skill execution** — inject mock SkillService, verify SkillRunner is wired (not Noop)
3. **Session::from_assembly() fidelity** — inject custom executor, run through dispatcher, verify it's called
4. **Invocation cloud-only** — verify no local path exists
5. **JsonSessionStore CRUD** — update existing tests for new layout (no session_id subdirectory)

## File Actions Summary

| Action | File |
|--------|------|
| Move | `src/local/assemble.rs` → `src/kernel/session/assembly/local.rs` |
| Move | `src/local/runtime.rs` → merged into assembly/local.rs |
| Edit | `src/local/cmd_run.rs` — import from kernel |
| Edit | `src/local/mod.rs` — remove assemble, runtime |
| Edit | `src/kernel/session/assembly/mod.rs` — add local |
| Edit | `src/kernel/invocation/request.rs` — remove AgentSource::Local |
| Edit | `src/kernel/invocation/session_route.rs` — remove local branch |
| Edit | `src/kernel/session/store/json.rs` — session_root semantics |
| Edit | `src/kernel/session/assembly/cloud.rs` — wire SkillRunner |
| Edit | `tests/unit/kernel/session/json_store.rs` — update for new layout |
| Edit | `tests/unit/local/mod.rs` — update for new imports |
| Add  | test: cloud skill executor wiring |
| Add  | test: no double-nesting in local layout |

## Execution Order

1 → 2 → 3 → 4 → 5 → 6 → 7
