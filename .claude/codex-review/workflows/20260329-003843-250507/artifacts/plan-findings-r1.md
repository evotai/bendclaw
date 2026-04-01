## Findings Responses

[CRITICAL-1] FIX
The plan adds `user_id` to the WHERE clause but still keys by `(name, user_id)`. Since skill names are not globally unique, this is still unsafe. Fix: key `touch_last_used` by `skill_id` (the immutable identity), not by name. Update the trait signature, SQL, and all callers accordingly.

[CRITICAL-2] FIX
The plan has no prune/invalidation step for the filesystem mirror when subscriptions are revoked or expire. Add explicit prune logic to the sync loop: after syncing active subscriptions, remove mirror files for skills no longer accessible. Sync failures must not silently leave stale files.

[HIGH-1] FIX
Subscription visibility must cover all read entry points: `get_skill`, resolve, and execute — not only `list()`. Update the plan to explicitly include subscription checks in every path that reads a skill by identity.

[HIGH-2] FIX
Replace `UNION ALL` with `UNION` (or add `DISTINCT`) to eliminate duplicate rows. Also add a deterministic `ORDER BY` key (e.g., `id`) to ensure stable pagination.

[HIGH-3] FIX
The subscription join must filter on active subscription state. Add `AND s.revoked = FALSE` (or equivalent active/expiry column) to every subscription join in both variables and skills queries.

[HIGH-4] FIX
The sync loop must prune mirror files for skills whose subscriptions have been removed or revoked. Define this as an explicit step: diff the set of currently-subscribed skills against mirrored files and delete the difference.

[HIGH-5] FIX
Since we are not constrained by compatibility, remove `agent_id` as a meaningful parameter from all four handlers. Route by user scope only (`ctx.user_id`). Eliminate the path param or make it a no-op placeholder with no semantic role.

[HIGH-6] FIX
Every subscription join must enforce both conditions: `v.scope = 'shared'` (resource-level visibility) AND `s.revoked = FALSE` (active subscription state). Neither condition alone is sufficient. Apply this consistently across variables and skills queries.

[MEDIUM-1] FIX
Change the predicate to `WHERE status = 'active' AND user_id IS NOT NULL AND user_id != ''`. Add a NOT NULL constraint to the `user_id` column in the schema to enforce this at the DB level.

[MEDIUM-2] FIX
Define explicit precedence: owned skills take priority over subscribed skills with the same name. Document this in the plan and implement it in `SkillService::list()` merge logic (e.g., deduplicate by name, keeping owned).

[MEDIUM-3] FIX
Add explicit integration test requirements to the plan: ownership isolation (user A cannot read user B's private skills), subscription revoke effects (revoked subscription removes access immediately), duplicate-name precedence, pagination stability under UNION, and sync prune correctness after unsubscribe.

[MEDIUM-4] FIX
Replace per-user subscription fetches with a single set-based query: fetch all (user_id, skill_id) subscription pairs in one query, then batch-sync. This eliminates N+1 and ensures consistency within a single transaction boundary.

[LOW-1] FIX
Centralize ownership + subscription visibility in a single repository method (or SQL CTE/view) per resource type, reused by list, get, and sync paths. This removes the ad hoc merge logic from `SkillService` and makes the access model a single auditable location.

[LOW-2] FIX
Since compatibility is not a constraint, remove `agent_id` path params entirely and replace with user-scoped routes. This eliminates the identity-boundary confusion at the routing layer.
