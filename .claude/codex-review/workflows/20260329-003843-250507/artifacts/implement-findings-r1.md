## Findings Responses

[CRITICAL-1] FIX
`runtime_builder.rs:390-393`: `list_user_ids()` failure silently falls back to `Vec::new()`, then `sync()` proceeds and calls `evict_stale()` on an empty set — potentially deleting all mirrored skills. Fix:
1. On `list_user_ids` error, log and skip the entire sync round (do not call `sync`).
2. Inside `sync()`, track per-user fetch failures; skip `evict_stale` for any user whose fetch failed, or fail-fast the whole round.
3. `list_subscribed` errors are currently silently swallowed (`Err(_) => {}`); at minimum log them and skip eviction for that user.

[CRITICAL-2] FIX
`sync.rs:38-45`: Subscribed skills overwrite `skill.user_id` with the subscriber's ID and use `(user_id, skill_name)` as the mirror key — colliding with owned skills of the same name. Fix:
1. Change directory structure to `.remote/subscribed/{owner_id}/{skill_name}` for subscribed skills, keeping `.remote/{skill_name}` for owned.
2. Update `live_keys` to use a 3-tuple `(subscriber, owner, skill_name)` or a namespaced path key.
3. Update `SkillCatalog::resolve` to apply explicit precedence: own > subscribed, deterministically.

[HIGH-1] FIX
`store.rs:156`: `UNION ALL` does not deduplicate. `workspace.rs:138-141`: `from_variables` collects into a `HashMap` with implicit last-write-wins, and `ORDER BY created_at DESC` does not guarantee own-first. Fix:
1. Change `UNION ALL` to `UNION` in `list_active` SQL.
2. In the service layer (before passing to `Workspace`), deduplicate by key with explicit own-first precedence: iterate owned variables first, then subscribed, inserting only if key is absent.

[MEDIUM-1] FIX
`runner.rs:144-160`: `touch_used_secret_variables` is called but `self.skills.touch_used(...)` is never called after execution. The `last_used_by` field on skills is never updated. Fix: after the successful execution branch (`exit_code == 0`), call `self.skills.touch_used(self.user_id.clone(), skill.name.clone(), self.agent_id.clone())`.

[MEDIUM-2] FIX
Routes remain `/agents/{agent_id}/skills` while handlers ignore `agent_id` entirely. Since compatibility is not a constraint, change routes to user-scoped paths (e.g., `/users/{user_id}/skills` or `/skills` with user derived from auth context). This eliminates the semantic split between route identity and actual authorization identity.

[LOW-1] FIX
`search.rs:297`: `store.touch_last_used(&id, "")` passes an empty string as actor. Use `ctx.agent_id` (or `ctx.user_id` if agent_id is absent) so `last_used_by` reflects the actual caller.
