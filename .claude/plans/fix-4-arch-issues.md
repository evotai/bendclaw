# Fix 4 architectural issues from review

## Issue 1 (Severe): SkillService write consistency

**Problem:** `catalog.insert()` returns `()`, silently discarding filesystem write failures. DB and mirror can diverge.

**Fix:**
- `catalog.insert()` → returns `Result<()>`, propagates `write_skill` failure
- `catalog.evict()` → returns `Result<()>`, propagates `remove_skill` failure
- `SkillService::create()` — if catalog write fails after DB write, call `store.remove()` to rollback, then return error
- `SkillService::delete()` — if catalog evict fails after DB delete, log warning (best-effort, DB is source of truth; sync loop will reconcile)

Files: `catalog.rs`, `service.rs`, `writer.rs` (change `write_skill` to return `Result`)

## Issue 2 (Severe): Org migrations never called

**Problem:** `run_org()` exists but is never invoked. The `evotai_meta` tables are never created.

**Fix:**
- In `runtime_builder.rs`, call `migrator::run_org(&meta_pool).await` right after creating `meta_pool` and before constructing `OrgServices`
- This runs once per Runtime startup, idempotent (`CREATE TABLE IF NOT EXISTS`)

Files: `runtime_builder.rs`

## Issue 3 (High): Prompt passes agent_id where user_id is needed

**Problem:** `append_skills()` receives `agent_id` but calls `for_user(agent_id)`. Skills are user-scoped, not agent-scoped.

**Fix:**
- `build()` line 289: change `self.append_skills(&mut prompt, agent_id)` → `self.append_skills(&mut prompt, user_id)`
- Rename the parameter in `append_skills` from `agent_id` to `user_id` for clarity
- Same check on `append_memory` — already correct (uses both)

Files: `prompt.rs`

## Issue 4 (Medium): OrgServices encapsulation + dead _catalog fields

**Problem:** Public fields make OrgServices a service locator. `_catalog` fields in tools are dead weight.

**Fix:**
- Make OrgServices fields private, add accessor methods: `skills()`, `variables()`, `memory()`, `subscriptions()`
- Remove `_catalog` from `SkillCreateTool` and `SkillRemoveTool` (they only need `service`)
- Update all call sites: `org.skills` → `org.skills()`, etc.

Files: `org.rs`, `session_factory.rs`, `commands.rs`, `registry.rs`, `create.rs`, `remove.rs`, `runtime.rs`, `run.rs`
