# Fix identity boundaries, touch_last_used, and subscription read path

## Issue 1: Sync loop uses agent_ids where user_ids are needed

**Problem:** `runtime_builder.rs:390` calls `databases.list_agent_ids()` which returns `agent_id` from `evotai_agents`, then passes them as `user_ids` to `sync::sync()`. Skills are stored by `user_id`, not `agent_id`.

**Fix:**
- Add `list_user_ids()` to `AgentDatabases` — `SELECT DISTINCT user_id FROM evotai_meta.evotai_agents WHERE status = 'active' AND user_id != ''`
- Change sync loop in `runtime_builder.rs` to call `list_user_ids()` instead of `list_agent_ids()`

Files: `src/storage/databases.rs`, `src/kernel/runtime/runtime_builder.rs`

## Issue 2: HTTP skill endpoints mix agent_id and user_id

**Problem:** `list_skills`, `get_skill`, `delete_skill` extract `agent_id` from URL path and pass it as `user_id` to the service. `create_skill` correctly uses `ctx.user_id`. Inconsistent ownership semantics.

**Fix:**
- All four handlers use `ctx.user_id` for the service call
- `agent_id` path param kept for routing compatibility but not used as ownership key
- `delete_skill` gets `ctx: RequestContext` (currently `_ctx`)

Files: `src/service/v1/skills/http.rs`

## Issue 3: touch_last_used missing user_id WHERE clause

**Problem:** `shared/store.rs:198` UPDATE has no `AND user_id = ?`, so same-named skills across users overwrite each other's `last_used_by`. Also, `runner.rs:220` passes empty string as agent_id.

**Fix:**
- Change `touch_last_used` signature: add `user_id` parameter
- Add `AND user_id = ?` to the WHERE clause
- Update `SharedSkillStore` trait to include `user_id` in `touch_last_used`
- Update `SkillService::touch_used()` to accept and pass `user_id`
- Update `SkillRunner` to pass actual `user_id` (it already has `self.user_id`)
- Update test mock implementations

Files: `src/kernel/skills/shared/store.rs`, `src/kernel/skills/shared/mod.rs`, `src/kernel/skills/service.rs`, `src/kernel/skills/runner.rs`, `crates/test-harness/src/mocks/skill.rs`

## Issue 4: Subscription read path — wire into variables and skills

**Problem:** `list_active` and `for_user` only return the user's own resources. Subscriptions exist but are never consulted at read time.

**Fix for variables:**
- Update `SharedVariableStore::list_active` SQL to UNION with subscribed variables:
  ```sql
  SELECT {COLS} FROM variables WHERE user_id = ? AND revoked = FALSE
  UNION ALL
  SELECT {COLS} FROM variables v
    INNER JOIN resource_subscriptions s
      ON s.resource_type = 'variable' AND s.resource_key = v.id AND s.user_id = ?
    WHERE v.scope = 'shared' AND v.user_id != ? AND v.revoked = FALSE
  ORDER BY created_at DESC LIMIT ?
  ```
- This requires the `VariableStore` to know about subscriptions, OR we do the join in SQL directly (simpler, no trait change needed — just update the SQL in `SharedVariableStore`)

**Fix for skills:**
- Add `list_subscribed` to `SharedSkillStore` trait — returns skills the user has subscribed to from other users
- `SkillService::list()` merges: catalog.for_user(user_id) + store.list_subscribed(user_id)
- Subscribed skills get synced to filesystem mirror in the sync loop (query subscriptions table for each user, fetch those skills too)
- OR simpler: `SharedVariableStore::list_active` does the JOIN in SQL; for skills, `SkillService::list()` queries subscribed skills from DB and merges with catalog results

**Chosen approach (simpler):**
- Variables: update SQL in `SharedVariableStore::list_active` to include subscribed variables via JOIN
- Skills: `SkillService::list()` calls `store.list_subscribed(user_id)` and merges with `catalog.for_user(user_id)`. The sync loop also syncs subscribed skills to the filesystem mirror.

Files: `src/kernel/variables/store.rs`, `src/kernel/skills/shared/store.rs`, `src/kernel/skills/service.rs`, `src/kernel/skills/remote/sync.rs`
