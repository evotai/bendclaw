# Fix identity boundaries, touch_last_used, and subscription read path

## Issue 1: Sync loop uses agent_ids where user_ids are needed

**Problem:** `runtime_builder.rs:390` calls `databases.list_agent_ids()` which returns `agent_id` from `evotai_agents`, then passes them as `user_ids` to `sync::sync()`. Skills are stored by `user_id`, not `agent_id`.

**Fix:**
- Add `list_user_ids()` to `AgentDatabases`:
  `SELECT DISTINCT user_id FROM evotai_meta.evotai_agents WHERE status = 'active' AND user_id IS NOT NULL AND user_id != ''`
- Add a NOT NULL constraint on `user_id` in the schema to enforce this at the DB level
- Change sync loop in `runtime_builder.rs` to call `list_user_ids()` instead of `list_agent_ids()`

Files: `src/storage/databases.rs`, `src/kernel/runtime/runtime_builder.rs`

## Issue 2: HTTP skill endpoints use agent_id as ownership key

**Problem:** `list_skills`, `get_skill`, `delete_skill` extract `agent_id` from URL path and pass it as `user_id` to the service. `create_skill` correctly uses `ctx.user_id`. Inconsistent ownership semantics.

**Fix:**
- Remove `agent_id` path params entirely; replace with user-scoped routes
- All four handlers use `ctx.user_id` exclusively for the service call
- `delete_skill` gets `ctx: RequestContext` (currently `_ctx`)

Files: `src/service/v1/skills/http.rs`

## Issue 3: touch_last_used keyed by name instead of skill_id

**Problem:** `shared/store.rs:198` UPDATE is keyed by `(name, user_id)`. Skill names are not globally unique, so same-named skills across users can collide. Also, `runner.rs:220` passes empty string as agent_id.

**Fix:**
- Change `touch_last_used` signature to accept `skill_id` (immutable identity) instead of `name`
- Update SQL WHERE clause to `WHERE id = ? AND user_id = ?`
- Update `SharedSkillStore` trait accordingly
- Update `SkillService::touch_used()` to accept and pass `skill_id` and `user_id`
- Update `SkillRunner` to pass actual `skill_id` and `user_id` (it already has `self.user_id`)
- Update test mock implementations

Files: `src/kernel/skills/shared/store.rs`, `src/kernel/skills/shared/mod.rs`, `src/kernel/skills/service.rs`, `src/kernel/skills/runner.rs`, `crates/test-harness/src/mocks/skill.rs`

## Issue 4: Subscription read path — centralized accessible-resources abstraction

**Problem:** `list_active` and `for_user` only return the user's own resources. Subscriptions exist but are never consulted at read time. Access logic is scattered across service and sync layers.

**Design principle:** Centralize ownership + subscription visibility in a single repository method (or SQL CTE) per resource type, reused by list, get, and sync paths. Every subscription join must enforce both:
- `scope = 'shared'` (resource-level visibility)
- `s.revoked = FALSE` (active subscription state)

**Fix for variables:**
- Update `SharedVariableStore::list_active` SQL to UNION owned + subscribed variables:
  ```sql
  SELECT {COLS} FROM variables WHERE user_id = ? AND revoked = FALSE
  UNION
  SELECT {COLS} FROM variables v
    INNER JOIN resource_subscriptions s
      ON s.resource_type = 'variable' AND s.resource_key = v.id AND s.user_id = ?
    WHERE v.scope = 'shared' AND v.user_id != ? AND v.revoked = FALSE AND s.revoked = FALSE
  ORDER BY id DESC LIMIT ?
  ```
  Use `UNION` (not `UNION ALL`) to deduplicate. Use `id` as the deterministic pagination key.

**Fix for skills:**
- Add a single `list_accessible(user_id)` method to `SharedSkillStore` that returns both owned and subscribed skills in one query (SQL CTE or UNION with `UNION` dedup)
- Owned skills take precedence over subscribed skills with the same name (deduplicate by name, keeping owned)
- `SkillService::list()` calls `store.list_accessible(user_id)` — no ad hoc merge in the service layer
- `SkillService::get_skill()`, resolve, and execute paths also use `list_accessible` or an equivalent single-query accessor — subscription visibility is enforced on every read entry point, not only listing

**Fix for sync loop:**
- Sync loop fetches all (user_id, skill_id) subscription pairs in a single set-based query (no per-user N+1)
- After syncing active subscriptions, prune mirror files for skills no longer accessible: diff currently-subscribed set against mirrored files and delete the difference
- Sync failures must not silently leave stale files; log and surface errors explicitly
- Authorization drift prevention: if prune fails, the sync loop must not mark the user's mirror as up-to-date

Files: `src/kernel/variables/store.rs`, `src/kernel/skills/shared/store.rs`, `src/kernel/skills/service.rs`, `src/kernel/skills/remote/sync.rs`

## Testing requirements

Integration tests must cover:
- Ownership isolation: user A cannot read user B's private skills or variables
- Subscription access: user A can read user B's shared skill after subscribing
- Revocation: revoking a subscription removes access immediately (DB query returns nothing, mirror is pruned)
- Duplicate-name precedence: owned skill wins over subscribed skill with same name
- Pagination stability: UNION queries return stable, deduplicated results across pages
- Sync prune correctness: unsubscribing removes the mirror file; sync failure does not leave stale files
- touch_last_used: keyed by skill_id, does not affect other users' skills with the same name
