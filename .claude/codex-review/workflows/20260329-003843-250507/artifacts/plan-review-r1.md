## Plan Review

### Correctness

[CRITICAL-1] `touch_last_used` keyed by `(name, user_id)` is still semantically unsafe.  
If skill names are not globally unique per owner, updates can hit multiple rows; if a subscriber uses another user’s skill, passing `self.user_id` may update nothing. This should be keyed by immutable skill identity (for example `skill_id` + owner) and usage attribution (`last_used_by`) should be modeled separately from ownership matching.

[HIGH-1] Subscription support is only planned for `SkillService::list()`, not the full skill read path.  
If `get_skill`/resolve/execute paths still query “owned only,” subscribed skills may appear in list but fail on read/execute. The plan should explicitly cover all read entry points, not only listing.

[HIGH-2] Variables query uses `UNION ALL` without dedupe guarantees.  
If duplicate subscription rows exist (or other duplication conditions), users can get duplicate variables, unstable ordering, and pagination anomalies. Use dedupe (`UNION` or `DISTINCT`) or enforce uniqueness in subscription schema plus deterministic pagination keys.

[MEDIUM-1] `list_user_ids()` predicate `user_id != ''` is incomplete.  
It does not guard `NULL`/whitespace values and may still emit invalid identities. Use explicit `IS NOT NULL` plus normalization constraints.

### Completeness

[HIGH-3] Revocation/expiry semantics for subscriptions are not included in query filters.  
The plan joins `resource_subscriptions` by type/key/user but does not mention `active`, `revoked`, or expiry columns. Missing these conditions can expose resources that should no longer be readable.

[HIGH-4] Sync-loop cleanup behavior is missing for unsubscribed/revoked resources.  
Plan adds syncing subscribed skills, but does not define pruning of mirror files after subscription removal. That can leave stale local artifacts visible/executable.

[MEDIUM-2] No explicit conflict policy for same skill name across owned and subscribed sets.  
`SkillService::list()` merge behavior needs deterministic precedence/order and duplicate handling, otherwise clients get ambiguous results.

[MEDIUM-3] Testing scope is underspecified for cross-tenant regressions.  
The plan mentions mock updates, but not required integration tests: ownership isolation, subscription revoke effects, duplicate-name behavior, pagination stability, and sync prune correctness.

### Risks

[CRITICAL-2] Authorization drift risk between DB truth and filesystem mirror is not addressed.  
If access decisions are DB-based but execution uses mirrored files, any lag or failed prune can become an access-control bypass window. Plan needs explicit invalidation/prune guarantees and failure handling.

[HIGH-5] Ignoring path `agent_id` entirely creates API contract ambiguity and audit risk.  
Even if ownership uses `ctx.user_id`, the handler should validate `agent_id` belongs to the caller (or deprecate/remove it). Silent ignore can hide client bugs and weaken operational auditability.

[MEDIUM-4] N+1/per-user subscription sync can become expensive and inconsistent under load.  
The plan implies per-user subscription fetches in sync. Prefer set-based queries with clear batching/transaction boundaries.

### Alternatives

[LOW-1] Prefer a single “accessible resources” query abstraction per resource type.  
Instead of merging in service + ad hoc sync logic, centralize ownership + subscription visibility in one repository query (or DB view/CTE) reused by list/get/sync paths.

[LOW-2] Consider replacing path `agent_id` routes with user-scoped routes over time.  
Keeping compatibility is fine short-term, but a deprecation plan reduces long-term identity-boundary confusion.

### Feasibility

Implementable, but it assumes:
- skill identity can be safely matched by name for updates (likely false),
- subscription lifecycle fields are either absent or unnecessary (risky),
- sync mirror can be extended without explicit prune/error model (incomplete).

Without addressing those assumptions, implementation is likely to compile but still produce correctness and access-control defects.

### Security

[HIGH-6] Missing explicit subscription-state checks is a direct authorization concern.  
Access should require both resource-level visibility (`scope='shared'`) and active subscription state (not revoked/expired). Both must be enforced in every read path and sync source query.

## Summary

- Critical: 2
- High: 6
- Medium: 4
- Low: 2

Overall: The plan identifies real boundary issues, but it is not yet safe to implement as-is. The biggest blockers are identity for `touch_last_used`, full read-path subscription enforcement (not just list), and mirror revocation/pruning guarantees.