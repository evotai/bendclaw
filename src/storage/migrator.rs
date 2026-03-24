use super::pool::Pool;
use crate::observability::log::slog;

/// Base migrations — independent CREATE TABLE IF NOT EXISTS files.
/// Files have no cross-dependencies and can run in parallel.
const BASE_MIGRATIONS: &[&str] = &[
    include_str!("../../migrations/base/sessions.sql"),
    include_str!("../../migrations/base/runs.sql"),
    include_str!("../../migrations/base/agent.sql"),
    include_str!("../../migrations/base/memory.sql"),
    include_str!("../../migrations/base/skills.sql"),
    include_str!("../../migrations/base/traces.sql"),
    include_str!("../../migrations/base/variables.sql"),
    include_str!("../../migrations/base/tasks.sql"),
    include_str!("../../migrations/base/feedback.sql"),
    include_str!("../../migrations/base/channels.sql"),
    include_str!("../../migrations/base/recall.sql"),
];

/// Alter migrations — ALTER TABLE, DROP, etc. that depend on base tables.
/// Executed strictly in order after all base migrations complete.
const ALTER_MIGRATIONS: &[&str] = &[include_str!(
    "../../migrations/alter/0001_runs_checkpoint_fields.sql"
)];

/// Run all agent migrations against the pool's current database.
pub async fn run_agent(pool: &Pool) {
    // Phase 1: base tables — files run in parallel (no cross-dependencies).
    let futs = BASE_MIGRATIONS
        .iter()
        .map(|sql| run_one_file(pool, sql, "base"));
    crate::base::runtime::join_bounded(futs, crate::base::runtime::CONCURRENCY_DB).await;
    slog!(
        info,
        "storage",
        "migrations_completed",
        scope = "base",
        count = BASE_MIGRATIONS.len(),
    );

    // Phase 2: alter — strict sequential for ordering guarantees.
    if !ALTER_MIGRATIONS.is_empty() {
        for sql in ALTER_MIGRATIONS {
            run_one_file(pool, sql, "alter").await;
        }
        slog!(
            info,
            "storage",
            "migrations_completed",
            scope = "alter",
            count = ALTER_MIGRATIONS.len(),
        );
    }
}

async fn run_one_file(pool: &Pool, sql: &str, scope: &str) {
    for stmt in sql.split(';').filter(|s| !s.trim().is_empty()) {
        let stmt = stmt.trim();
        if let Err(e) = pool.exec(stmt).await {
            slog!(debug, "storage", "migration_skipped", scope, error = %e,);
        }
    }
}
