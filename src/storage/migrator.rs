use super::pool::Pool;
use crate::observability::log::slog;

/// Base migrations — the only executable schema source.
/// Files have no cross-dependencies and can run in parallel.
const BASE_MIGRATIONS: &[&str] = &[
    include_str!("../../migrations/base/sessions.sql"),
    include_str!("../../migrations/base/runs.sql"),
    include_str!("../../migrations/base/agent.sql"),
    include_str!("../../migrations/base/traces.sql"),
    include_str!("../../migrations/base/tasks.sql"),
    include_str!("../../migrations/base/feedback.sql"),
    include_str!("../../migrations/base/channels.sql"),
    include_str!("../../migrations/base/recall.sql"),
];

/// Org-level migrations — registry tables in evotai_meta database.
const ORG_MIGRATIONS: &[&str] = &[
    include_str!("../../migrations/org/agents.sql"),
    include_str!("../../migrations/org/memory.sql"),
    include_str!("../../migrations/org/variables.sql"),
    include_str!("../../migrations/org/skills.sql"),
    include_str!("../../migrations/org/subscriptions.sql"),
];

/// Run all agent migrations against the pool's current database.
pub async fn run_agent(pool: &Pool) {
    let futs = BASE_MIGRATIONS
        .iter()
        .map(|sql| run_one_file(pool, sql, "base"));
    crate::types::runtime::join_bounded(futs, crate::types::runtime::CONCURRENCY_DB).await;
    slog!(
        info,
        "storage",
        "migrations_completed",
        scope = "base",
        count = BASE_MIGRATIONS.len(),
    );
}

/// Run org-level migrations (evotai_meta database).
pub async fn run_org(pool: &Pool) {
    let futs = ORG_MIGRATIONS
        .iter()
        .map(|sql| run_one_file(pool, sql, "org"));
    crate::types::runtime::join_bounded(futs, crate::types::runtime::CONCURRENCY_DB).await;
    slog!(
        info,
        "storage",
        "org_migrations_completed",
        scope = "org",
        count = ORG_MIGRATIONS.len(),
    );
}

async fn run_one_file(pool: &Pool, sql: &str, _scope: &str) {
    for stmt in sql.split(';').filter(|s| !s.trim().is_empty()) {
        let stmt = stmt.trim();
        let _ = pool.exec(stmt).await;
    }
}
